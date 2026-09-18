// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::{
    errors::{FeatureBuildStage, FeatureGraphWarning},
    graph::{
        DepRequiredOrOptional, DependencyReq, FeatureIndexInPackage, FeatureIx, NamedFeatureDep,
        PackageGraph, PackageIx, PackageLink, PackageMetadata,
        feature::{
            ConditionalLinkImpl, EnabledLink, FeatureEdge, FeatureGraphImpl, FeatureLabel,
            FeatureMetadataImpl, FeatureNode, LinkDeclarations, PackageEdgeIxs, SlashForm,
            WeakDependencies, WeakSlashImpl,
        },
    },
    platform::PlatformStatusImpl,
};
use ahash::AHashMap;
use cargo_metadata::DependencyKind;
use once_cell::sync::OnceCell;
use petgraph::{prelude::*, visit::IntoEdgeReferences};
use smallvec::SmallVec;
use std::{iter, slice};

pub(super) type FeaturePetgraph = Graph<FeatureNode, FeatureEdge, Directed, FeatureIx>;
pub(super) type FeatureEdgeReference<'g> = <&'g FeaturePetgraph as IntoEdgeReferences>::EdgeRef;

#[derive(Debug)]
pub(super) struct FeatureGraphBuildState {
    graph: FeaturePetgraph,
    // Map from package ixs to the base (first) feature for each package.
    base_ixs: Vec<NodeIndex<FeatureIx>>,
    map: AHashMap<FeatureNode, FeatureMetadataImpl>,
    weak: WeakDependencies,
    warnings: Vec<FeatureGraphWarning>,
}

impl FeatureGraphBuildState {
    pub(super) fn new(package_graph: &PackageGraph) -> Self {
        let package_count = package_graph.package_count();
        Self {
            // Each package corresponds to at least one feature ID.
            graph: Graph::with_capacity(package_count, package_count),
            // Each package corresponds to exactly one base feature ix, and there's one last ix at
            // the end.
            base_ixs: Vec::with_capacity(package_count + 1),
            map: AHashMap::with_capacity(package_count),
            weak: WeakDependencies::new(),
            warnings: vec![],
        }
    }

    /// Add nodes for every feature in this package + the base package, and add edges from every
    /// feature to the base package.
    pub(super) fn add_nodes(&mut self, package: PackageMetadata<'_>) {
        let base_node = FeatureNode::base(package.package_ix());
        let base_ix = self.add_node(base_node);
        self.base_ixs.push(base_ix);
        FeatureNode::named_features(package)
            .chain(FeatureNode::optional_deps(package))
            .for_each(|feature_node| {
                let feature_ix = self.add_node(feature_node);
                self.graph
                    .update_edge(feature_ix, base_ix, FeatureEdge::FeatureToBase);
            });
    }

    /// Mark the end of adding nodes.
    pub(super) fn end_nodes(&mut self) {
        self.base_ixs.push(NodeIndex::new(self.graph.node_count()));
    }

    pub(super) fn add_named_feature_edges(&mut self, metadata: PackageMetadata<'_>) {
        // A dependency name can map to more than one link. Cargo and guppy
        // both unify instances that resolve to the same package, so this only
        // happens when a rename makes two *different* packages share one name
        // -- e.g. semver 1.0.28's `serde = { package = "serde_core" }`
        // alongside a plain `serde` under `cfg(any())`. Both are part of
        // `dep:serde` and `serde/...`, so keying by name alone would silently
        // drop one.
        let mut dep_name_to_links: AHashMap<&str, SmallVec<[PackageLink; 1]>> = AHashMap::new();
        for link in metadata.direct_links() {
            dep_name_to_links
                .entry(link.dep_name())
                .or_default()
                .push(link);
        }

        metadata
            .named_features_full()
            .for_each(|(n, from_feature, feature_deps)| {
                let from_node = FeatureNode::new(metadata.package_ix(), n);

                let to_nodes_edges: Vec<_> = feature_deps
                    .iter()
                    .flat_map(|feature_dep| {
                        self.nodes_for_named_feature_dep(
                            metadata,
                            from_feature,
                            feature_dep,
                            &dep_name_to_links,
                        )
                    })
                    // The flat_map above holds an &mut reference to self, which is why it needs to
                    // be collected.
                    .collect();

                // Don't create a map to the base 'from' node since it is already created in
                // add_nodes.
                self.add_edges(from_node, to_nodes_edges, metadata.graph());
            })
    }

    fn nodes_for_named_feature_dep(
        &mut self,
        metadata: PackageMetadata<'_>,
        from_named_feature: &str,
        feature_dep: &NamedFeatureDep,
        dep_name_to_links: &AHashMap<&str, SmallVec<[PackageLink<'_>; 1]>>,
    ) -> SmallVec<[(FeatureNode, FeatureEdge); 3]> {
        let from_label = FeatureLabel::Named(from_named_feature);
        let mut nodes_edges: SmallVec<[(FeatureNode, FeatureEdge); 3]> = SmallVec::new();

        match feature_dep {
            NamedFeatureDep::DependencyNamedFeature {
                dep_name,
                feature,
                weak,
            } => {
                let links = dep_name_to_links
                    .get(dep_name.as_ref())
                    .map_or(&[][..], SmallVec::as_slice);

                // A cross-package edge lands on a different node per link, so
                // emit one per link.
                //
                // If the dependency is weak, each link also gets its own weak
                // index.
                for link in links {
                    let slash = if *weak {
                        self.make_weak_slash_impl(&metadata, link)
                    } else {
                        SlashForm::Strong
                    };

                    // Dependency from (`main`, `a`) to (`dep, `foo`)
                    if let Some(cross_node) = self.make_named_feature_node(
                        &metadata,
                        from_label,
                        &link.to(),
                        FeatureLabel::Named(feature.as_ref()),
                        true,
                    ) {
                        // This is a cross-package link. The platform-specific
                        // requirements still apply, so grab them from the
                        // PackageLink.
                        nodes_edges.push((
                            cross_node,
                            Self::make_named_feature_cross_edge(slice::from_ref(link), slash),
                        ));
                    };
                }

                // The edges below land on the same node for every link, so they
                // get one edge covering all of them.
                if !links.is_empty() {
                    // If the package is present as an optional dependency, it is
                    // implicitly activated by the feature:
                    // from (`main`, `a`) to (`main`, `dep:dep`)
                    //
                    // But this is skipped for weak `dep?/foo`, which never
                    // activates `dep:dep`. If we stored these kinds of edges,
                    // while doing a forward traversal we'd let a required
                    // declaration's edge release it and spuriously activate
                    // `dep:dep` (and thus the optional declaration). The weak
                    // cross edge above is sufficient on its own.
                    if !*weak
                        && let Some(same_node) = self.make_named_feature_node(
                            &metadata,
                            from_label,
                            &metadata,
                            FeatureLabel::OptionalDependency(dep_name),
                            // Don't warn if this dep isn't optional.
                            false,
                        )
                    {
                        nodes_edges.push((
                            same_node,
                            Self::make_named_feature_cross_edge(links, SlashForm::Strong),
                        ));
                    }

                    // Finally, (`main`, `a`) to (`main`, `dep`) -- if this is a non-weak dependency
                    // and a named feature by this name is present, it also gets activated (even if
                    // the named feature has no relation to the optional dependency).
                    //
                    // For example:
                    //
                    // server = ["hyper/server"]
                    //
                    // will also activate the named feature `hyper`.
                    //
                    // One thing to be careful of here is that we don't want to insert self-edges.
                    // For example:
                    //
                    // tokio = ["dep:tokio", "tokio/net"]
                    //
                    // should not insert a self-edge from `tokio` to `tokio`. The second condition
                    // checks this.
                    if !*weak
                        && &**dep_name != from_named_feature
                        && let Some(same_named_feature_node) = self.make_named_feature_node(
                            &metadata,
                            from_label,
                            &metadata,
                            FeatureLabel::Named(dep_name),
                            // Don't warn if this dep isn't optional.
                            false,
                        )
                    {
                        nodes_edges.push((
                            same_named_feature_node,
                            Self::make_named_feature_cross_edge(links, SlashForm::Strong),
                        ));
                    }
                }
            }
            NamedFeatureDep::NamedFeature(feature_name) => {
                if let Some(same_node) = self.make_named_feature_node(
                    &metadata,
                    from_label,
                    &metadata,
                    FeatureLabel::Named(feature_name.as_ref()),
                    true,
                ) {
                    nodes_edges.push((same_node, FeatureEdge::NamedFeature));
                }
            }
            NamedFeatureDep::OptionalDependency(dep_name) => {
                if let Some(same_node) = self.make_named_feature_node(
                    &metadata,
                    from_label,
                    &metadata,
                    FeatureLabel::OptionalDependency(dep_name.as_ref()),
                    true,
                ) && let Some(links) = dep_name_to_links.get(dep_name.as_ref())
                {
                    nodes_edges.push((
                        same_node,
                        FeatureEdge::NamedFeatureDepColon(Self::make_full_conditional_link_impl(
                            links,
                        )),
                    ));
                }
            }
        };

        nodes_edges
    }

    fn make_named_feature_node(
        &mut self,
        from_package: &PackageMetadata<'_>,
        from_label: FeatureLabel<'_>,
        to_package: &PackageMetadata<'_>,
        to_label: FeatureLabel<'_>,
        warn: bool,
    ) -> Option<FeatureNode> {
        match to_package.get_feature_idx(to_label) {
            Some(idx) => Some(FeatureNode::new(to_package.package_ix(), idx)),
            None => {
                // It is possible to specify a feature that doesn't actually exist, and cargo will
                // accept that if the feature isn't resolved. One example is the cfg-if crate, where
                // version 0.1.9 has the `rustc-dep-of-std` feature commented out, and several
                // crates try to enable that feature:
                // https://github.com/alexcrichton/cfg-if/issues/22
                //
                // Since these aren't fatal errors, it seems like the best we can do is to store
                // such issues as warnings.
                if warn {
                    self.warnings.push(FeatureGraphWarning::MissingFeature {
                        stage: FeatureBuildStage::AddNamedFeatureEdges {
                            package_id: from_package.id().clone(),
                            from_feature: from_label.to_string(),
                        },
                        package_id: to_package.id().clone(),
                        feature_name: to_label.to_string(),
                    });
                }
                None
            }
        }
    }

    /// Creates the cross link for situations like:
    ///
    /// ```toml
    /// [features]
    /// a = ["dep/foo"]
    /// ```
    ///
    /// (a link (`from`, `a`) to (`dep`, `foo`) is created.
    ///
    /// If `dep` is optional and the reference is not weak, the edge (`from`, `a`)
    /// to (`from`, `dep`) is also a `NamedFeatureWithSlash` edge.
    fn make_named_feature_cross_edge(links: &[PackageLink<'_>], slash: SlashForm) -> FeatureEdge {
        // This edge is enabled if the feature is enabled, which means the union of (required,
        // optional) build conditions.
        FeatureEdge::NamedFeatureWithSlash {
            link: Self::make_full_conditional_link_impl(links),
            slash,
        }
    }

    /// Returns [`SlashForm::Strong`] if the dependency has no optional
    /// declarations -- in that case, `foo?/b` behaves like `foo/b`, so modeling
    /// weak dependencies isn't required.
    fn make_weak_slash_impl(
        &mut self,
        metadata: &PackageMetadata<'_>,
        link: &PackageLink<'_>,
    ) -> SlashForm {
        let optional = EnabledLink::new(Self::make_conditional_link_impl(
            slice::from_ref(link),
            LinkDeclarations::Optional,
            |req| req.inner.optional.build_if.clone(),
        ));
        let Some(optional) = optional else {
            return SlashForm::Strong;
        };

        let required = EnabledLink::new(Self::make_conditional_link_impl(
            slice::from_ref(link),
            LinkDeclarations::Required,
            |req| req.inner.required.build_if.clone(),
        ));
        let optional_dependency_ix = metadata
            .get_feature_idx(FeatureLabel::OptionalDependency(link.dep_name()))
            .and_then(|idx| self.lookup_node(&FeatureNode::new(metadata.package_ix(), idx)))
            .unwrap_or_else(|| {
                panic!(
                    "for package '{}', optional declarations of '{}' have a dep: feature node",
                    metadata.id(),
                    link.dep_name(),
                )
            });
        SlashForm::Weak(Box::new(WeakSlashImpl {
            required,
            optional,
            index: self.weak.insert(link.edge_ix(), optional_dependency_ix),
        }))
    }

    // Creates a "full" conditional link, unifying requirements across all
    // dependency lines -- and, where a rename makes one dependency name
    // resolve to several packages, across every link for that name. Every
    // link's edge index is recorded, so `package_links` reports all of them.
    //
    // This should not be used in add_dependency_edges below!
    fn make_full_conditional_link_impl(links: &[PackageLink<'_>]) -> ConditionalLinkImpl {
        // This edge is enabled if the feature is enabled, which means the union of (required,
        // optional) build conditions.
        Self::make_conditional_link_impl(links, LinkDeclarations::Unsplit, |req| {
            let mut required = req.inner.required.build_if.clone();
            required.extend(&req.inner.optional.build_if);
            required
        })
    }

    fn make_conditional_link_impl<'g>(
        links: &[PackageLink<'g>],
        declarations: LinkDeclarations,
        status: impl Fn(DependencyReq<'g>) -> PlatformStatusImpl,
    ) -> ConditionalLinkImpl {
        let (first, rest) = links.split_first().expect("at least one link");
        let mut combined = ConditionalLinkImpl {
            package_edge_ixs: PackageEdgeIxs::single(first.edge_ix()),
            declarations,
            normal: status(first.normal()),
            build: status(first.build()),
            dev: status(first.dev()),
        };
        for link in rest {
            combined.package_edge_ixs.push(link.edge_ix());
            combined.normal.extend(&status(link.normal()));
            combined.build.extend(&status(link.build()));
            combined.dev.extend(&status(link.dev()));
        }
        combined
    }

    pub(super) fn add_dependency_edges(&mut self, link: PackageLink<'_>) {
        let from = link.from();

        // Sometimes the same package is depended on separately in different sections like so:
        //
        // bar/Cargo.toml:
        //
        // [dependencies]
        // foo = { version = "1", features = ["a"] }
        //
        // [build-dependencies]
        // foo = { version = "1", features = ["b"] }
        //
        // Now if you have a crate 'baz' with:
        //
        // [dependencies]
        // bar = { path = "../bar" }
        //
        // ... what features would you expect foo to be built with? You might expect it to just
        // be built with "a", but as it turns out Cargo actually *unifies* the features, such
        // that foo is built with both "a" and "b".
        //
        // Also, feature unification is impacted by whether the dependency is optional.
        //
        // [dependencies]
        // foo = { version = "1", features = ["a"] }
        //
        // [build-dependencies]
        // foo = { version = "1", optional = true, features = ["b"] }
        //
        // This will include 'foo' as a normal dependency but *not* as a build dependency by
        // default.
        // * Without '--features foo', the `foo` dependency will be built with "a".
        // * With '--features foo', `foo` will be both a normal and a build dependency, with
        //   features "a" and "b" in both instances.
        //
        // This means that up to two separate edges have to be represented:
        // * a 'required edge', which will be from the base node for 'from' to the feature nodes
        //   for each required feature in 'to'.
        // * an 'optional edge', which will be from the feature node (from, dep_name) to the
        //   feature nodes for each optional feature in 'to'. This edge is only added if at least
        //   one line is optional.

        let unified_metadata = iter::once((DependencyKind::Normal, link.normal()))
            .chain(iter::once((DependencyKind::Build, link.build())))
            .chain(iter::once((DependencyKind::Development, link.dev())));

        let mut required_req = FeatureReq::new(link, LinkDeclarations::Required);
        let mut optional_req = FeatureReq::new(link, LinkDeclarations::Optional);
        for (kind, dependency_req) in unified_metadata {
            required_req.add_features(kind, &dependency_req.inner.required, &mut self.warnings);
            optional_req.add_features(kind, &dependency_req.inner.optional, &mut self.warnings);
        }

        // Add the required edges (base -> features).
        self.add_edges(
            FeatureNode::base(from.package_ix()),
            required_req.finish(),
            link.from().graph(),
        );

        if !optional_req.is_empty() {
            // This means that there is at least one instance of this dependency with optional =
            // true. The dep name should have been added as an optional dependency node to the
            // package metadata.
            let from_node = FeatureNode::new(
                from.package_ix(),
                from.get_feature_idx(FeatureLabel::OptionalDependency(link.dep_name()))
                    .unwrap_or_else(|| {
                        panic!(
                        "while adding feature edges, for package '{}', optional dep '{}' missing",
                        from.id(),
                        link.dep_name(),
                    );
                    }),
            );
            self.add_edges(from_node, optional_req.finish(), link.from().graph());
        }
    }

    fn add_node(&mut self, feature_id: FeatureNode) -> NodeIndex<FeatureIx> {
        let feature_ix = self.graph.add_node(feature_id);
        self.map
            .insert(feature_id, FeatureMetadataImpl { feature_ix });
        feature_ix
    }

    fn add_edges(
        &mut self,
        from_node: FeatureNode,
        to_nodes_edges: impl IntoIterator<Item = (FeatureNode, FeatureEdge)>,
        graph: &PackageGraph,
    ) {
        // The from node should always be present because it is a known node.
        let from_ix = self.lookup_node(&from_node).unwrap_or_else(|| {
            panic!("while adding feature edges, missing 'from': {from_node:?}");
        });

        let to_nodes_edges = to_nodes_edges.into_iter().collect::<Vec<_>>();

        to_nodes_edges.into_iter().for_each(|(to_node, edge)| {
            let to_ix = self
                .lookup_node(&to_node)
                .unwrap_or_else(|| panic!("while adding feature edges, missing 'to': {to_node:?}"));

            if from_ix == to_ix {
                // Only named-feature self-loops are user errors -- e.g.,
                // `[features] a = ["a"]`. Self-loops arising from a
                // package's dependency declarations (such as a `path`
                // dev-dependency on the package's own crate) are
                // legitimate Cargo constructs and must not produce a
                // warning.
                let is_named_feature_loop = match &edge {
                    FeatureEdge::NamedFeature
                    | FeatureEdge::NamedFeatureDepColon(_)
                    | FeatureEdge::NamedFeatureWithSlash { .. } => true,
                    FeatureEdge::DependenciesSection(_) | FeatureEdge::FeatureToBase => false,
                };
                if is_named_feature_loop {
                    let (package_id, feature_label) = from_node.package_id_and_feature_label(graph);
                    self.warnings.push(FeatureGraphWarning::SelfLoop {
                        package_id: package_id.clone(),
                        feature_name: feature_label.to_string(),
                    });
                }
            }

            match self.graph.find_edge(from_ix, to_ix) {
                Some(edge_ix) => {
                    // The edge already exists. This could be an upgrade from a cross link to a
                    // feature dependency, for example:
                    //
                    // [package]
                    // name = "main"
                    //
                    // [dependencies]
                    // dep = { ..., optional = true }
                    //
                    // [features]
                    // "feat" = ["dep/feat", "dep"]
                    //
                    // "dep/feat" causes a cross link to be created from "main/feat" to "main/dep".
                    // However, the "dep" encountered later upgrades this link to a feature
                    // dependency.
                    //
                    // This could also be an upgrade from a weak to a non-weak dependency:
                    //
                    // [features]
                    // feat = ["dep?/feat", "dep/feat2"]
                    let old_edge = self
                        .graph
                        .edge_weight_mut(edge_ix)
                        .expect("this edge was just found");
                    match (old_edge, edge) {
                        (
                            FeatureEdge::NamedFeatureWithSlash {
                                slash: old_slash, ..
                            },
                            FeatureEdge::NamedFeatureWithSlash { slash, .. },
                        ) => {
                            if let (SlashForm::Weak(old_weak), SlashForm::Weak(weak)) =
                                (&*old_slash, &slash)
                            {
                                debug_assert_eq!(
                                    old_weak.index, weak.index,
                                    "weak indexes should match if both are weak"
                                );
                            }
                            // Upgrade this edge from weak to non-weak.
                            match slash {
                                SlashForm::Strong => *old_slash = SlashForm::Strong,
                                SlashForm::Weak(_) => {}
                            }
                        }
                        (
                            old_edge @ FeatureEdge::NamedFeatureWithSlash { .. },
                            edge @ FeatureEdge::NamedFeature
                            | edge @ FeatureEdge::NamedFeatureDepColon(_),
                        ) => {
                            // Upgrade this edge from / conditional to dep: conditional or unconditional.
                            *old_edge = edge;
                        }
                        (
                            old_edge @ FeatureEdge::NamedFeatureDepColon(_),
                            edge @ FeatureEdge::NamedFeature,
                        ) => {
                            // Upgrade this edge from dep: conditional to unconditional.
                            // XXX: can this ever happen?
                            *old_edge = edge;
                        }

                        _ => {
                            // In all other cases, leave the old edge alone.
                        }
                    }
                }
                None => {
                    self.graph.add_edge(from_ix, to_ix, edge);
                }
            }
        })
    }

    fn lookup_node(&self, node: &FeatureNode) -> Option<NodeIndex<FeatureIx>> {
        self.map.get(node).map(|metadata| metadata.feature_ix)
    }

    pub(super) fn build(self) -> FeatureGraphImpl {
        FeatureGraphImpl {
            graph: self.graph,
            base_ixs: self.base_ixs,
            map: self.map,
            warnings: self.warnings,
            sccs: OnceCell::new(),
            weak: self.weak,
        }
    }
}

#[derive(Debug)]
struct FeatureReq<'g> {
    link: PackageLink<'g>,
    declarations: LinkDeclarations,
    to: PackageMetadata<'g>,
    edge_ix: EdgeIndex<PackageIx>,
    to_default_idx: FeatureIndexInPackage,
    // This will contain any build states that aren't empty.
    features: AHashMap<FeatureIndexInPackage, DependencyBuildState>,
}

impl<'g> FeatureReq<'g> {
    fn new(link: PackageLink<'g>, declarations: LinkDeclarations) -> Self {
        let to = link.to();
        Self {
            link,
            declarations,
            to,
            edge_ix: link.edge_ix(),
            to_default_idx: to
                .get_feature_idx(FeatureLabel::Named("default"))
                .unwrap_or(FeatureIndexInPackage::Base),
            features: AHashMap::new(),
        }
    }

    fn is_empty(&self) -> bool {
        // self.features only consists of non-empty build states.
        self.features.is_empty()
    }

    fn add_features(
        &mut self,
        dep_kind: DependencyKind,
        req: &DepRequiredOrOptional,
        warnings: &mut Vec<FeatureGraphWarning>,
    ) {
        // Base feature.
        self.extend(FeatureIndexInPackage::Base, dep_kind, &req.build_if);
        // Default feature (or base if it isn't present).
        self.extend(self.to_default_idx, dep_kind, &req.default_features_if);

        for (feature, status) in &req.feature_targets {
            match self.to.get_feature_idx(FeatureLabel::Named(feature)) {
                Some(feature_idx) => {
                    self.extend(feature_idx, dep_kind, status);
                }
                None => {
                    // The destination feature is missing -- this is accepted by cargo
                    // in some circumstances, so use a warning rather than an error.
                    warnings.push(FeatureGraphWarning::MissingFeature {
                        stage: FeatureBuildStage::AddDependencyEdges {
                            package_id: self.link.from().id().clone(),
                            dep_name: self.link.dep_name().to_string(),
                        },
                        package_id: self.to.id().clone(),
                        feature_name: feature.to_string(),
                    });
                }
            }
        }
    }

    fn extend(
        &mut self,
        feature_idx: FeatureIndexInPackage,
        dep_kind: DependencyKind,
        status: &PlatformStatusImpl,
    ) {
        let package_edge_ix = self.edge_ix;
        let declarations = self.declarations;
        if !status.is_never() {
            self.features
                .entry(feature_idx)
                .or_insert_with(|| DependencyBuildState::new(package_edge_ix, declarations))
                .extend(dep_kind, status);
        }
    }

    fn finish(self) -> impl Iterator<Item = (FeatureNode, FeatureEdge)> + use<> {
        let package_ix = self.to.package_ix();
        self.features
            .into_iter()
            .map(move |(feature_idx, build_state)| {
                // extend ensures that the build states aren't empty. Double-check that.
                debug_assert!(
                    !build_state.link.is_never(),
                    "build states are always non-empty"
                );
                (
                    FeatureNode::new(package_ix, feature_idx),
                    build_state.finish(),
                )
            })
    }
}

#[derive(Debug)]
struct DependencyBuildState {
    link: ConditionalLinkImpl,
}

impl DependencyBuildState {
    fn new(package_edge_ix: EdgeIndex<PackageIx>, declarations: LinkDeclarations) -> Self {
        Self {
            link: ConditionalLinkImpl {
                package_edge_ixs: PackageEdgeIxs::single(package_edge_ix),
                declarations,
                normal: PlatformStatusImpl::default(),
                build: PlatformStatusImpl::default(),
                dev: PlatformStatusImpl::default(),
            },
        }
    }

    fn extend(&mut self, dep_kind: DependencyKind, status: &PlatformStatusImpl) {
        match dep_kind {
            DependencyKind::Normal => self.link.normal.extend(status),
            DependencyKind::Build => self.link.build.extend(status),
            DependencyKind::Development => self.link.dev.extend(status),
            _ => panic!("unknown dependency kind"),
        }
    }

    fn finish(self) -> FeatureEdge {
        FeatureEdge::DependenciesSection(self.link)
    }
}
