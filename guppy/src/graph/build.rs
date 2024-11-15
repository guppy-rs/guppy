// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::{
    graph::{
        cargo_version_matches, BuildTargetImpl, BuildTargetKindImpl, DepRequiredOrOptional,
        DependencyReqImpl, NamedFeatureDep, OwnedBuildTargetId, PackageGraph, PackageGraphData,
        PackageIx, PackageLinkImpl, PackageMetadataImpl, PackagePublishImpl, PackageSourceImpl,
        WorkspaceImpl,
    },
    sorted_set::SortedSet,
    Error, PackageId,
};
use ahash::AHashMap;
use camino::{Utf8Path, Utf8PathBuf};
use cargo_metadata::{
    DepKindInfo, Dependency, DependencyKind, Metadata, Node, NodeDep, Package, Target,
};
use fixedbitset::FixedBitSet;
use indexmap::{IndexMap, IndexSet};
use once_cell::sync::OnceCell;
use petgraph::prelude::*;
use semver::{Version, VersionReq};
use smallvec::SmallVec;
use std::{
    borrow::Cow,
    cell::RefCell,
    collections::{BTreeMap, HashSet},
    rc::Rc,
};
use target_spec::TargetSpec;

impl PackageGraph {
    /// Constructs a new `PackageGraph` instances from the given metadata.
    pub(crate) fn build(mut metadata: Metadata) -> Result<Self, Box<Error>> {
        // resolve_nodes is missing if the metadata was generated with --no-deps.
        let resolve_nodes = metadata.resolve.map(|r| r.nodes).unwrap_or_default();

        let workspace_members: HashSet<_> = metadata
            .workspace_members
            .into_iter()
            .map(PackageId::from_metadata)
            .collect();

        let workspace_root = metadata.workspace_root;

        let mut build_state = GraphBuildState::new(
            &mut metadata.packages,
            resolve_nodes,
            &workspace_root,
            &workspace_members,
        )?;

        let packages: AHashMap<_, _> = metadata
            .packages
            .into_iter()
            .map(|package| build_state.process_package(package))
            .collect::<Result<_, _>>()?;

        let dep_graph = build_state.finish();

        let workspace = WorkspaceImpl::new(
            workspace_root,
            metadata.target_directory,
            metadata.workspace_metadata,
            &packages,
            workspace_members,
        )?;

        Ok(Self {
            dep_graph,
            sccs: OnceCell::new(),
            feature_graph: OnceCell::new(),
            data: PackageGraphData {
                packages,
                workspace,
            },
        })
    }
}

impl WorkspaceImpl {
    /// Indexes and creates a new workspace.
    fn new(
        workspace_root: impl Into<Utf8PathBuf>,
        target_directory: impl Into<Utf8PathBuf>,
        metadata_table: serde_json::Value,
        packages: &AHashMap<PackageId, PackageMetadataImpl>,
        members: impl IntoIterator<Item = PackageId>,
    ) -> Result<Self, Box<Error>> {
        use std::collections::btree_map::Entry;

        let workspace_root = workspace_root.into();
        // Build up the workspace members by path, since most interesting queries are going to
        // happen by path.
        let mut members_by_path = BTreeMap::new();
        let mut members_by_name = BTreeMap::new();
        for id in members {
            // Strip off the workspace path from the manifest path.
            let package_metadata = packages.get(&id).ok_or_else(|| {
                Error::PackageGraphConstructError(format!("workspace member '{}' not found", id))
            })?;

            let workspace_path = match &package_metadata.source {
                PackageSourceImpl::Workspace(path) => path,
                _ => {
                    return Err(Error::PackageGraphConstructError(format!(
                        "workspace member '{}' at path {:?} not in workspace",
                        id, package_metadata.manifest_path,
                    ))
                    .into());
                }
            };
            members_by_path.insert(workspace_path.to_path_buf(), id.clone());

            match members_by_name.entry(package_metadata.name.clone().into_boxed_str()) {
                Entry::Vacant(vacant) => {
                    vacant.insert(id.clone());
                }
                Entry::Occupied(occupied) => {
                    return Err(Error::PackageGraphConstructError(format!(
                        "duplicate package name in workspace: '{}' is name for '{}' and '{}'",
                        occupied.key(),
                        occupied.get(),
                        id
                    ))
                    .into());
                }
            }
        }

        Ok(Self {
            root: workspace_root,
            target_directory: target_directory.into(),
            metadata_table,
            members_by_path,
            members_by_name,
            #[cfg(feature = "proptest1")]
            name_list: OnceCell::new(),
        })
    }
}

/// Helper struct for building up dependency graph.
struct GraphBuildState<'a> {
    dep_graph: Graph<PackageId, PackageLinkImpl, Directed, PackageIx>,
    package_data: AHashMap<PackageId, Rc<PackageDataValue>>,
    // The above, except by package name.
    by_package_name: AHashMap<String, Vec<Rc<PackageDataValue>>>,

    // The values of resolve_data are the resolved dependencies. This is mutated so it is stored
    // separately from package_data.
    resolve_data: AHashMap<PackageId, Vec<NodeDep>>,
    workspace_root: &'a Utf8Path,
    workspace_members: &'a HashSet<PackageId>,
}

impl<'a> GraphBuildState<'a> {
    /// This method drains the list of targets from the package.
    fn new(
        packages: &mut [Package],
        resolve_nodes: Vec<Node>,
        workspace_root: &'a Utf8Path,
        workspace_members: &'a HashSet<PackageId>,
    ) -> Result<Self, Box<Error>> {
        // Precomputing the edge count is a roughly 5% performance improvement.
        let edge_count = resolve_nodes
            .iter()
            .map(|node| node.deps.len())
            .sum::<usize>();

        let mut dep_graph = Graph::with_capacity(packages.len(), edge_count);
        let all_package_data: AHashMap<_, _> = packages
            .iter_mut()
            .map(|package| PackageDataValue::new(package, &mut dep_graph))
            .collect::<Result<_, _>>()?;

        // While it is possible to have duplicate names so the hash map is smaller, just make this
        // as big as package_data.
        let mut by_package_name: AHashMap<String, Vec<Rc<PackageDataValue>>> =
            AHashMap::with_capacity(all_package_data.len());
        for package_data in all_package_data.values() {
            by_package_name
                .entry(package_data.name.clone())
                .or_default()
                .push(package_data.clone());
        }

        let resolve_data: AHashMap<_, _> = resolve_nodes
            .into_iter()
            .map(|node| {
                (
                    PackageId::from_metadata(node.id),
                    // This used to return resolved features (node.features) as well but guppy
                    // now does its own feature handling, so it isn't used any more.
                    node.deps,
                )
            })
            .collect();

        Ok(Self {
            dep_graph,
            package_data: all_package_data,
            by_package_name,
            resolve_data,
            workspace_root,
            workspace_members,
        })
    }

    fn process_package(
        &mut self,
        package: Package,
    ) -> Result<(PackageId, PackageMetadataImpl), Box<Error>> {
        let package_id = PackageId::from_metadata(package.id);
        let (package_data, build_targets) =
            self.package_data_and_remove_build_targets(&package_id)?;

        let source = if self.workspace_members.contains(&package_id) {
            PackageSourceImpl::Workspace(self.workspace_path(&package_id, &package.manifest_path)?)
        } else if let Some(source) = package.source {
            if source.is_crates_io() {
                PackageSourceImpl::CratesIo
            } else {
                PackageSourceImpl::External(source.repr.into())
            }
        } else {
            // Path dependency: get the directory from the manifest path.
            let dirname = match package.manifest_path.parent() {
                Some(dirname) => dirname,
                None => {
                    return Err(Error::PackageGraphConstructError(format!(
                        "package '{}': manifest path '{}' does not have parent",
                        package_id, package.manifest_path,
                    ))
                    .into());
                }
            };
            PackageSourceImpl::create_path(dirname, self.workspace_root)
        };

        // resolved_deps is missing if the metadata was generated with --no-deps.
        let resolved_deps = self.resolve_data.remove(&package_id).unwrap_or_default();

        let dep_resolver = DependencyResolver::new(
            &package_id,
            &self.package_data,
            &self.by_package_name,
            &package.dependencies,
        );

        for NodeDep {
            name: resolved_name,
            pkg,
            dep_kinds,
            ..
        } in resolved_deps
        {
            let dep_id = PackageId::from_metadata(pkg);
            let (dep_data, deps) = dep_resolver.resolve(&resolved_name, &dep_id, &dep_kinds)?;
            let link = PackageLinkImpl::new(&package_id, &resolved_name, deps)?;
            // Use update_edge instead of add_edge to prevent multiple edges from being added
            // between these two nodes.
            // XXX maybe check for an existing edge?
            self.dep_graph
                .update_edge(package_data.package_ix, dep_data.package_ix, link);
        }

        let has_default_feature = package.features.contains_key("default");

        // Optional dependencies could in principle be computed by looking at the edges out of this
        // package, but unresolved dependencies aren't part of the graph so we're going to miss them
        // (and many optional dependencies will be unresolved).
        //
        // XXX: Consider modeling unresolved dependencies in the graph.
        //
        // A dependency might be listed multiple times (e.g. as a build dependency and as a normal
        // one). Some of them might be optional, some might not be. List a dependency here if *any*
        // of those specifications are optional, since that's how Cargo features work. But also
        // dedup them.
        let optional_deps: IndexSet<_> = package
            .dependencies
            .into_iter()
            .filter_map(|dep| {
                if dep.optional {
                    match dep.rename {
                        Some(rename) => Some(rename.into_boxed_str()),
                        None => Some(dep.name.into_boxed_str()),
                    }
                } else {
                    None
                }
            })
            .collect();

        // Has the explicit feature by the name of this optional dep been seen?
        let mut seen_explicit = FixedBitSet::with_capacity(optional_deps.len());

        // The feature map contains both optional deps and named features.
        let mut named_features: IndexMap<_, _> = package
            .features
            .into_iter()
            .map(|(feature_name, deps)| {
                let mut parsed_deps = SmallVec::with_capacity(deps.len());
                for dep in deps {
                    let dep = NamedFeatureDep::from_cargo_string(dep);
                    if let NamedFeatureDep::OptionalDependency(d) = &dep {
                        let index = optional_deps.get_index_of(d.as_ref()).ok_or_else(|| {
                            Error::PackageGraphConstructError(format!(
                                "package '{}': named feature {} specifies 'dep:{d}', but {d} is not an optional dependency",
                                package_id,
                                feature_name,
                                d = d))
                        })?;
                        seen_explicit.set(index, true);
                    }
                    parsed_deps.push(dep);
                }
                Ok((feature_name.into_boxed_str(), parsed_deps))
            })
            .collect::<Result<_, Error>>()?;

        // If an optional dependency was not seen explicitly, add an implicit named feature for it.
        for (index, dep) in optional_deps.iter().enumerate() {
            if !seen_explicit.contains(index) {
                named_features.insert(
                    dep.clone(),
                    std::iter::once(NamedFeatureDep::OptionalDependency(dep.clone())).collect(),
                );
            }
        }

        // For compatibility with previous versions of guppy -- remove when a breaking change
        // occurs.
        let rust_version_req = package
            .rust_version
            .as_ref()
            .map(|rust_version| VersionReq {
                comparators: vec![semver::Comparator {
                    op: semver::Op::GreaterEq,
                    major: rust_version.major,
                    minor: Some(rust_version.minor),
                    patch: Some(rust_version.patch),
                    // Rust versions don't support pre-release fields.
                    pre: semver::Prerelease::EMPTY,
                }],
            });

        Ok((
            package_id,
            PackageMetadataImpl {
                name: package.name,
                version: package.version,
                authors: package.authors,
                description: package.description.map(|s| s.into()),
                license: package.license.map(|s| s.into()),
                license_file: package.license_file.map(|f| f.into()),
                manifest_path: package.manifest_path.into(),
                categories: package.categories,
                keywords: package.keywords,
                readme: package.readme.map(|s| s.into()),
                repository: package.repository.map(|s| s.into()),
                homepage: package.homepage.map(|s| s.into()),
                documentation: package.documentation.map(|s| s.into()),
                edition: package.edition.to_string().into_boxed_str(),
                metadata_table: package.metadata,
                links: package.links.map(|s| s.into()),
                publish: PackagePublishImpl::new(package.publish),
                default_run: package.default_run.map(|s| s.into()),
                rust_version: package.rust_version,
                rust_version_req,
                named_features,
                optional_deps,

                package_ix: package_data.package_ix,
                source,
                build_targets,
                has_default_feature,
            },
        ))
    }

    fn package_data_and_remove_build_targets(
        &self,
        id: &PackageId,
    ) -> Result<(Rc<PackageDataValue>, BuildTargetMap), Box<Error>> {
        let package_data = self.package_data.get(id).ok_or_else(|| {
            Error::PackageGraphConstructError(format!("no package data found for package '{}'", id))
        })?;
        let package_data = package_data.clone();
        let build_targets = std::mem::take(&mut *package_data.build_targets.borrow_mut());
        Ok((package_data, build_targets))
    }

    /// Computes the workspace path for this package. Errors if this package is not in the
    /// workspace.
    fn workspace_path(
        &self,
        id: &PackageId,
        manifest_path: &Utf8Path,
    ) -> Result<Box<Utf8Path>, Box<Error>> {
        // Try to strip off the workspace path from the manifest path.
        let _utf8_path_buf; // relative path lifetime helper
        let workspace_path = if let Ok(stripped_workspace_path) =
            manifest_path.strip_prefix(self.workspace_root)
        {
            stripped_workspace_path
        } else {
            // Error::PackageGraphConstructError(format!(
            //     "workspace member '{}' at path {} not in workspace (root: {})",
            //     id, manifest_path, self.workspace_root
            // ));
            // If manifest path is out of workspace root, try find relative path instead
            _utf8_path_buf = find_relative_path_utf8(self.workspace_root, manifest_path);
            _utf8_path_buf.as_path()
        };
        let workspace_path = workspace_path.parent().ok_or_else(|| {
            Error::PackageGraphConstructError(format!(
                "workspace member '{}' has invalid manifest path {:?}",
                id, manifest_path
            ))
        })?;
        Ok(convert_forward_slashes(workspace_path).into_boxed_path())
    }

    fn finish(self) -> Graph<PackageId, PackageLinkImpl, Directed, PackageIx> {
        self.dep_graph
    }
}

/// Intermediate state for a package as stored in `GraphBuildState`.
#[derive(Debug)]
struct PackageDataValue {
    package_ix: NodeIndex<PackageIx>,
    name: String,
    resolved_name: ResolvedName,
    // build_targets is used in two spots: in the constructor here, and removed from this field in
    // package_data_and_remove_build_targets.
    build_targets: RefCell<BuildTargetMap>,
    version: Version,
}

impl PackageDataValue {
    fn new(
        package: &mut Package,
        dep_graph: &mut Graph<PackageId, PackageLinkImpl, Directed, PackageIx>,
    ) -> Result<(PackageId, Rc<Self>), Box<Error>> {
        let package_id = PackageId::from_metadata(package.id.clone());
        let package_ix = dep_graph.add_node(package_id.clone());

        // Build up the list of build targets -- this will be used to construct the resolved_name.
        let mut build_targets = BuildTargets::new(&package_id);
        for build_target in package.targets.drain(..) {
            build_targets.add(build_target)?;
        }
        let build_targets = build_targets.finish();

        let resolved_name = match build_targets.get(&OwnedBuildTargetId::Library) {
            Some(target) => {
                let lib_name = target
                    .lib_name
                    .as_deref()
                    .expect("lib_name is always specified for library targets");
                if lib_name != package.name {
                    ResolvedName::LibNameSpecified(lib_name.to_string())
                } else {
                    // The resolved name is the same as the package name.
                    ResolvedName::LibNameNotSpecified(lib_name.replace('-', "_"))
                }
            }
            None => {
                // This means that it's a weird case like a binary-only dependency (not part of
                // stable Rust as of 2023-11). This will typically be reflected as an empty resolved
                // name.
                ResolvedName::NoLibTarget
            }
        };

        let value = PackageDataValue {
            package_ix,
            name: package.name.clone(),
            resolved_name,
            build_targets: RefCell::new(build_targets),
            version: package.version.clone(),
        };

        Ok((package_id, Rc::new(value)))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
enum ResolvedName {
    LibNameSpecified(String),
    /// This variant has its - replaced with _.
    LibNameNotSpecified(String),
    NoLibTarget,
}

/// Matcher for the resolved name of a dependency.
///
/// The "rename" field in a dependency, if present, is generally used. (But not always! There are
/// cases where even if a rename is present, the package name is used instead.)
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
struct ReqResolvedName<'g> {
    // A renamed name, if any.
    renamed: Option<String>,

    // A resolved name created from the lib.name field.
    resolved_name: &'g ResolvedName,
}

impl<'g> ReqResolvedName<'g> {
    fn new(renamed: Option<&str>, resolved_name: &'g ResolvedName) -> Self {
        Self {
            renamed: renamed.map(|s| s.replace('-', "_")),
            resolved_name,
        }
    }

    fn matches(&self, name: &str) -> bool {
        if let Some(rename) = &self.renamed {
            if rename == name {
                return true;
            }
        }

        match self.resolved_name {
            ResolvedName::LibNameSpecified(resolved_name) => *resolved_name == name,
            ResolvedName::LibNameNotSpecified(resolved_name) => *resolved_name == name,
            ResolvedName::NoLibTarget => {
                // This code path is only hit with nightly Rust as of 2023-11. It depends on Rust
                // RFC 3028. at https://github.com/rust-lang/cargo/issues/9096.
                //
                // This isn't quite right -- if we have two or more non-lib dependencies, we'll
                // return true for both of them over here. What we need to do instead is use the
                // extern_name and bin_name fields that are present in nightly DepKindInfo, but that
                // aren't in stable yet. For now, this is the best we can do.
                //
                // (If we're going to be relying on heuristics, it is also possible to use the
                // package ID over here, but that's documented to be an opaque string. It also
                // wouldn't be resilient to patch and replace.)
                name.is_empty()
            }
        }
    }
}

impl PackageSourceImpl {
    fn create_path(path: &Utf8Path, workspace_root: &Utf8Path) -> Self {
        let path_diff =
            pathdiff::diff_utf8_paths(path, workspace_root).expect("workspace root is absolute");
        // On Windows, the directory name and the workspace root might be on different drives,
        // in which case the path can't be relative.
        let path_diff = if path_diff.is_absolute() {
            path_diff
        } else {
            convert_forward_slashes(path_diff)
        };
        Self::Path(path_diff.into_boxed_path())
    }
}

impl NamedFeatureDep {
    fn from_cargo_string(input: impl Into<String>) -> Self {
        let input = input.into();
        match input.split_once('/') {
            Some((dep_name, feature)) => {
                if let Some(dep_name_without_q) = dep_name.strip_suffix('?') {
                    Self::dep_named_feature(dep_name_without_q, feature, true)
                } else {
                    Self::dep_named_feature(dep_name, feature, false)
                }
            }
            None => match input.strip_prefix("dep:") {
                Some(dep_name) => Self::optional_dependency(dep_name),
                None => Self::named_feature(input),
            },
        }
    }
}

type BuildTargetMap = BTreeMap<OwnedBuildTargetId, BuildTargetImpl>;

struct BuildTargets<'a> {
    package_id: &'a PackageId,
    targets: BuildTargetMap,
}

impl<'a> BuildTargets<'a> {
    fn new(package_id: &'a PackageId) -> Self {
        Self {
            package_id,
            targets: BTreeMap::new(),
        }
    }

    fn add(&mut self, target: Target) -> Result<(), Box<Error>> {
        use std::collections::btree_map::Entry;

        // Figure out the id and kind using target.kind and target.crate_types.
        let mut target_kinds = target.kind;
        let target_name = target.name.into_boxed_str();
        let crate_types = SortedSet::new(target.crate_types);

        // The "proc-macro" crate type cannot mix with any other types or kinds.
        if target_kinds.len() > 1 && Self::is_proc_macro(&target_kinds) {
            return Err(Error::PackageGraphConstructError(format!(
                "for package {}, proc-macro mixed with other kinds ({:?})",
                self.package_id, target_kinds
            ))
            .into());
        }
        if crate_types.len() > 1 && Self::is_proc_macro(&crate_types) {
            return Err(Error::PackageGraphConstructError(format!(
                "for package {}, proc-macro mixed with other crate types ({})",
                self.package_id, crate_types
            ))
            .into());
        }

        let (id, kind, lib_name) = if target_kinds.len() > 1 {
            // multiple kinds always means a library target.
            (
                OwnedBuildTargetId::Library,
                BuildTargetKindImpl::LibraryOrExample(crate_types),
                Some(target_name),
            )
        } else if let Some(target_kind) = target_kinds.pop() {
            let (id, lib_name) = match target_kind.as_str() {
                "custom-build" => (OwnedBuildTargetId::BuildScript, Some(target_name)),
                "bin" => (OwnedBuildTargetId::Binary(target_name), None),
                "example" => (OwnedBuildTargetId::Example(target_name), None),
                "test" => (OwnedBuildTargetId::Test(target_name), None),
                "bench" => (OwnedBuildTargetId::Benchmark(target_name), None),
                _other => {
                    // Assume that this is a library crate.
                    (OwnedBuildTargetId::Library, Some(target_name))
                }
            };

            let kind = match &id {
                OwnedBuildTargetId::Library => {
                    if crate_types.as_slice() == ["proc-macro"] {
                        BuildTargetKindImpl::ProcMacro
                    } else {
                        BuildTargetKindImpl::LibraryOrExample(crate_types)
                    }
                }
                OwnedBuildTargetId::Example(_) => {
                    BuildTargetKindImpl::LibraryOrExample(crate_types)
                }
                _ => {
                    // The crate_types must be exactly "bin".
                    if crate_types.as_slice() != ["bin"] {
                        return Err(Error::PackageGraphConstructError(format!(
                            "for package {}: build target '{:?}' has invalid crate types '{}'",
                            self.package_id, id, crate_types,
                        ))
                        .into());
                    }
                    BuildTargetKindImpl::Binary
                }
            };

            (id, kind, lib_name)
        } else {
            return Err(Error::PackageGraphConstructError(format!(
                "for package ID '{}': build target '{}' has no kinds",
                self.package_id, target_name
            ))
            .into());
        };

        match self.targets.entry(id) {
            Entry::Occupied(occupied) => {
                return Err(Error::PackageGraphConstructError(format!(
                    "for package ID '{}': duplicate build targets for {:?}",
                    self.package_id,
                    occupied.key()
                ))
                .into());
            }
            Entry::Vacant(vacant) => {
                vacant.insert(BuildTargetImpl {
                    kind,
                    lib_name,
                    required_features: target.required_features,
                    path: target.src_path.into_boxed_path(),
                    edition: target.edition.to_string().into_boxed_str(),
                    doc_tests: target.doctest,
                });
            }
        }

        Ok(())
    }

    fn is_proc_macro(list: &[String]) -> bool {
        list.iter().any(|kind| kind.as_str() == "proc-macro")
    }

    fn finish(self) -> BuildTargetMap {
        self.targets
    }
}

struct DependencyResolver<'g> {
    from_id: &'g PackageId,

    /// The package data, inherited from the graph build state.
    package_data: &'g AHashMap<PackageId, Rc<PackageDataValue>>,

    /// This is a list of dependency requirements. We don't know the package ID yet so we don't have
    /// a great key to work with. This could be improved in the future by matching on requirements
    /// (though it's hard).
    dep_reqs: DependencyReqs<'g>,
}

impl<'g> DependencyResolver<'g> {
    /// Constructs a new resolver using the provided package data and dependencies.
    fn new(
        from_id: &'g PackageId,
        package_data: &'g AHashMap<PackageId, Rc<PackageDataValue>>,
        by_package_name: &'g AHashMap<String, Vec<Rc<PackageDataValue>>>,
        package_deps: impl IntoIterator<Item = &'g Dependency>,
    ) -> Self {
        let mut dep_reqs = DependencyReqs::default();
        for dep in package_deps {
            // Determine what the resolved name of each package could be by matching on package name
            // and version (NOT source, because the source can be patched).
            let Some(packages) = by_package_name.get(&dep.name) else {
                // This dependency did not lead to a resolved package.
                continue;
            };
            for package in packages {
                if cargo_version_matches(&dep.req, &package.version) {
                    // The cargo `resolve.deps` map uses one of two things:
                    //
                    // 1. dep.rename with - turned into _, if specified.
                    // 2. lib.name, if specified, otherwise package.name with - turned into _.
                    //
                    // ReqResolvedName tracks both of these.
                    let req_resolved_name =
                        ReqResolvedName::new(dep.rename.as_deref(), &package.resolved_name);
                    dep_reqs.push(req_resolved_name, dep);
                }
            }
        }

        Self {
            from_id,
            package_data,
            dep_reqs,
        }
    }

    /// Resolves this dependency by finding the `Dependency` items corresponding to this resolved
    /// name and package ID.
    fn resolve<'a>(
        &'a self,
        resolved_name: &'a str,
        dep_id: &PackageId,
        dep_kinds: &'a [DepKindInfo],
    ) -> Result<
        (
            &'g Rc<PackageDataValue>,
            impl Iterator<Item = &'g Dependency> + 'a,
        ),
        Error,
    > {
        let dep_data = self.package_data.get(dep_id).ok_or_else(|| {
            Error::PackageGraphConstructError(format!(
                "{}: no package data found for dependency '{}'",
                self.from_id, dep_id
            ))
        })?;

        Ok((
            dep_data,
            self.dep_reqs
                .matches_for(resolved_name, dep_data, dep_kinds),
        ))
    }
}

/// Maintains a list of dependency requirements to match up to for a given package name.
#[derive(Clone, Debug, Default)]
struct DependencyReqs<'g> {
    // The keys are (resolved name, dependency).
    reqs: Vec<(ReqResolvedName<'g>, &'g Dependency)>,
}

impl<'g> DependencyReqs<'g> {
    fn push(&mut self, resolved_name: ReqResolvedName<'g>, dependency: &'g Dependency) {
        self.reqs.push((resolved_name, dependency));
    }

    fn matches_for<'a>(
        &'a self,
        resolved_name: &'a str,
        package_data: &'a PackageDataValue,
        dep_kinds: &'a [DepKindInfo],
    ) -> impl Iterator<Item = &'g Dependency> + 'a {
        self.reqs
            .iter()
            .filter_map(move |(req_resolved_name, dep)| {
                // A dependency requirement matches this package if all of the following are true:
                //
                // 1. The resolved_name matches.
                // 2. The Cargo version matches (XXX is this necessary?)
                // 3. The dependency kind and target is found in dep_kinds.
                if !req_resolved_name.matches(resolved_name) {
                    return None;
                }

                if !cargo_version_matches(&dep.req, &package_data.version) {
                    return None;
                }

                // Some older manifests don't have the dep_kinds field -- in that case we can't
                // fully match manifests and just accept all such packages. We just can't do better
                // than that.
                if dep_kinds.is_empty() {
                    return Some(*dep);
                }

                dep_kinds
                    .iter()
                    .any(|dep_kind| dep_kind.kind == dep.kind && dep_kind.target == dep.target)
                    .then_some(*dep)
            })
    }
}

impl PackageLinkImpl {
    fn new<'a>(
        from_id: &PackageId,
        resolved_name: &str,
        deps: impl IntoIterator<Item = &'a Dependency>,
    ) -> Result<Self, Box<Error>> {
        let mut version_req = None;
        let mut normal = DependencyReqImpl::default();
        let mut build = DependencyReqImpl::default();
        let mut dev = DependencyReqImpl::default();

        // We hope that the dep name is the same for all of these, but it's not guaranteed.
        let mut dep_name: Option<String> = None;
        for dep in deps {
            let rename_or_name = dep.rename.as_ref().unwrap_or(&dep.name);
            match &dep_name {
                Some(dn) => {
                    if dn != rename_or_name {
                        // XXX: warn or error on this?
                    }
                }
                None => {
                    dep_name = Some(rename_or_name.clone());
                }
            }

            // Dev dependencies cannot be optional.
            if dep.kind == DependencyKind::Development && dep.optional {
                return Err(Error::PackageGraphConstructError(format!(
                    "for package '{}': dev-dependency '{}' marked optional",
                    from_id,
                    dep_name.expect("dep_name set above"),
                ))
                .into());
            }

            // Pick the first version req that this come across.
            if version_req.is_none() {
                version_req = Some(dep.req.clone());
            }

            match dep.kind {
                DependencyKind::Normal => normal.add_instance(from_id, dep)?,
                DependencyKind::Build => build.add_instance(from_id, dep)?,
                DependencyKind::Development => dev.add_instance(from_id, dep)?,
                _ => {
                    // unknown dependency kind -- can't do much with this!
                    continue;
                }
            };
        }

        let dep_name = dep_name.ok_or_else(|| {
            Error::PackageGraphConstructError(format!(
                "for package '{}': no dependencies found matching '{}'",
                from_id, resolved_name,
            ))
        })?;
        let version_req = version_req.unwrap_or_else(|| {
            panic!(
                "requires at least one dependency instance: \
                 from `{from_id}` to `{dep_name}` (resolved name `{resolved_name}`)"
            )
        });

        Ok(Self {
            dep_name,
            resolved_name: resolved_name.into(),
            version_req,
            normal,
            build,
            dev,
        })
    }
}

/// It is possible to specify a dependency several times within the same section through
/// platform-specific dependencies and the [target] section. For example:
/// https://github.com/alexcrichton/flate2-rs/blob/5751ad9/Cargo.toml#L29-L33
///
/// ```toml
/// [dependencies]
/// miniz_oxide = { version = "0.3.2", optional = true}
///
/// [target.'cfg(all(target_arch = "wasm32", not(target_os = "emscripten")))'.dependencies]
/// miniz_oxide = "0.3.2"
/// ```
///
/// (From here on, each separate time a particular version of a dependency
/// is listed, it is called an "instance".)
///
/// For such situations, there are two separate analyses that happen:
///
/// 1. Whether the dependency is included at all. This is a union of all instances, conditional on
///    the specifics of the `[target]` lines.
/// 2. What features are enabled. As of cargo 1.42, this is unified across all instances but
///    separately for required/optional instances.
///
/// Note that the new feature resolver
/// (https://doc.rust-lang.org/nightly/cargo/reference/unstable.html#features)'s `itarget` setting
/// causes this union-ing to *not* happen, so that's why we store all the features enabled by
/// each target separately.
impl DependencyReqImpl {
    fn add_instance(&mut self, from_id: &PackageId, dep: &Dependency) -> Result<(), Box<Error>> {
        if dep.optional {
            self.optional.add_instance(from_id, dep)
        } else {
            self.required.add_instance(from_id, dep)
        }
    }
}

impl DepRequiredOrOptional {
    fn add_instance(&mut self, from_id: &PackageId, dep: &Dependency) -> Result<(), Box<Error>> {
        // target_spec is None if this is not a platform-specific dependency.
        let target_spec = match dep.target.as_ref() {
            Some(spec_or_triple) => {
                // This is a platform-specific dependency, so add it to the list of specs.
                let spec_or_triple = format!("{}", spec_or_triple);
                let target_spec: TargetSpec = spec_or_triple.parse().map_err(|err| {
                    Error::PackageGraphConstructError(format!(
                        "for package '{}': for dependency '{}', parsing target '{}' failed: {}",
                        from_id, dep.name, spec_or_triple, err
                    ))
                })?;
                Some(target_spec)
            }
            None => None,
        };

        self.build_if.add_spec(target_spec.as_ref());
        if dep.uses_default_features {
            self.default_features_if.add_spec(target_spec.as_ref());
        } else {
            self.no_default_features_if.add_spec(target_spec.as_ref());
        }

        for feature in &dep.features {
            self.feature_targets
                .entry(feature.clone())
                .or_default()
                .add_spec(target_spec.as_ref());
        }
        Ok(())
    }
}

impl PackagePublishImpl {
    /// Converts cargo_metadata registries to our own format.
    fn new(registries: Option<Vec<String>>) -> Self {
        match registries {
            None => PackagePublishImpl::Unrestricted,
            Some(registries) => PackagePublishImpl::Registries(registries.into_boxed_slice()),
        }
    }
}

/// Replace backslashes in a relative path with forward slashes on Windows.
#[track_caller]
fn convert_forward_slashes<'a>(rel_path: impl Into<Cow<'a, Utf8Path>>) -> Utf8PathBuf {
    let rel_path = rel_path.into();
    debug_assert!(
        rel_path.is_relative(),
        "path {} should be relative",
        rel_path,
    );

    cfg_if::cfg_if! {
        if #[cfg(windows)] {
            rel_path.as_str().replace("\\", "/").into()
        } else {
            rel_path.into_owned()
        }
    }
}

// Calculate the relative path from `from` to `to`.
// This function finds the relative path between two given paths.
// It first identifies the common prefix between the two paths and then
// constructs the relative path by adding ".." for each remaining component
// in the `from` path and appending the remaining components from the `to` path.
#[track_caller]
pub fn find_relative_path_utf8(from: &Utf8Path, to: &Utf8Path) -> Utf8PathBuf {
    let from_path = from;
    let to_path = to;

    let mut from_components = from_path.components();
    let mut to_components = to_path.components();

    // Initialize an empty Utf8PathBuf to store the relative path
    let mut relative_path = Utf8PathBuf::new();

    // Iterate through the components of both paths to find the common prefix
    while let (Some(f), Some(t)) = (from_components.next(), to_components.next()) {
        if f != t {
            // If the components differ, add ".." for each remaining component in the `from` path
            relative_path.push("..");
            let from_remaining = from_components.as_path();
            for _ in from_remaining.components() {
                relative_path.push("..");
            }
            // Add the current component from the `to` path
            relative_path.push(t);
            break;
        }
    }

    // Append the remaining components from the `to` path
    relative_path.extend(to_components.as_path().components());
    relative_path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_named_feature_dependency() {
        assert_eq!(
            NamedFeatureDep::from_cargo_string("dep/bar"),
            NamedFeatureDep::dep_named_feature("dep", "bar", false),
        );
        assert_eq!(
            NamedFeatureDep::from_cargo_string("dep?/bar"),
            NamedFeatureDep::dep_named_feature("dep", "bar", true),
        );
        assert_eq!(
            NamedFeatureDep::from_cargo_string("dep:bar"),
            NamedFeatureDep::optional_dependency("bar"),
        );
        assert_eq!(
            NamedFeatureDep::from_cargo_string("foo-bar"),
            NamedFeatureDep::named_feature("foo-bar"),
        );
    }

    #[test]
    fn test_create_path() {
        assert_eq!(
            PackageSourceImpl::create_path("/data/foo".as_ref(), "/data/bar".as_ref()),
            PackageSourceImpl::Path("../foo".into())
        );
        assert_eq!(
            PackageSourceImpl::create_path("/tmp/foo".as_ref(), "/data/bar".as_ref()),
            PackageSourceImpl::Path("../../tmp/foo".into())
        );
    }

    #[cfg(windows)]
    #[test]
    fn test_create_path_windows() {
        // Ensure that relative paths are stored with forward slashes.
        assert_eq!(
            PackageSourceImpl::create_path("C:\\data\\foo".as_ref(), "C:\\data\\bar".as_ref()),
            PackageSourceImpl::Path("../foo".into())
        );
        // Paths that span drives cannot be stored as relative.
        assert_eq!(
            PackageSourceImpl::create_path("D:\\tmp\\foo".as_ref(), "C:\\data\\bar".as_ref()),
            PackageSourceImpl::Path("D:\\tmp\\foo".into())
        );
    }

    #[test]
    fn test_convert_forward_slashes() {
        let components = vec!["..", "..", "foo", "bar", "baz.txt"];
        let path: Utf8PathBuf = components.into_iter().collect();
        let path = convert_forward_slashes(path);
        // This should have forward slashes, even on Windows.
        assert_eq!(path.as_str(), "../../foo/bar/baz.txt");
    }

    #[test]
    fn test_workspace_path_out_of_pocket() {
        let path_workspace_root = "/workspace/a/b/.cargo/workspace";
        let path_manifest = "/workspace/a/b/Crate/Cargo.toml";

        let expected_relative_path = r"../../Crate/Cargo.toml";

        let relative_path = find_relative_path_utf8(
            Utf8Path::new(path_workspace_root),
            Utf8Path::new(path_manifest),
        );
        assert_eq!(convert_forward_slashes(relative_path), expected_relative_path);
    }
}
