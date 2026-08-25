// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Generate build summaries from `CargoSet` instances.
//!
//! Requires the `summaries` feature to be enabled.

mod package_set;

use crate::{
    Error,
    graph::{
        DependencyDirection, PackageGraph, PackageMetadata, PackageSet, PackageSource,
        cargo::{
            BuildPlatform, CargoOptions, CargoResolverVersion, CargoSet, CargoSetInputs,
            InitialsPlatform,
        },
        feature::{FeatureId, FeatureSet},
    },
    platform::PlatformSpecSummary,
};
pub use guppy_summaries::*;
pub use package_set::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

impl CargoSet<'_> {
    /// Creates a build summary for this `CargoSet`.
    ///
    /// Requires the `summaries` feature to be enabled.
    pub fn to_summary(&self) -> Result<Summary, Error> {
        let initials = self.initials();
        let metadata = CargoSetInputsSummary::new(self.inputs())?;
        let target_features = self.target_features();
        let host_features = self.host_features();

        let mut summary = Summary::with_metadata(&metadata).map_err(Error::TomlSerializeError)?;
        summary.target_packages =
            target_features.to_package_map(initials, self.target_direct_deps());
        summary.host_packages = host_features.to_package_map(initials, self.host_direct_deps());

        Ok(summary)
    }
}

impl<'g> FeatureSet<'g> {
    /// Creates a `PackageMap` from this `FeatureSet`.
    ///
    /// `initials` and `direct_deps` are used to assign a PackageStatus.
    fn to_package_map(
        &self,
        initials: &FeatureSet<'g>,
        direct_deps: &PackageSet<'g>,
    ) -> PackageMap {
        self.packages_with_features(DependencyDirection::Forward)
            .map(|feature_list| {
                let package = feature_list.package();

                let status = if initials.contains_package_ix(package.package_ix()) {
                    PackageStatus::Initial
                } else if package.in_workspace() {
                    PackageStatus::Workspace
                } else if direct_deps.contains_ix(package.package_ix()) {
                    PackageStatus::Direct
                } else {
                    PackageStatus::Transitive
                };

                let info = PackageInfo {
                    status,
                    features: feature_list
                        .named_features()
                        .map(|feature| feature.to_owned())
                        .collect(),
                    optional_deps: feature_list
                        .optional_deps()
                        .map(|dep| dep.to_owned())
                        .collect(),
                };

                (feature_list.package().to_summary_id(), info)
            })
            .collect()
    }
}

impl PackageGraph {
    /// Converts this `SummaryId` to a `PackageMetadata`.
    ///
    /// Returns an error if the summary ID could not be matched.
    ///
    /// Requires the `summaries` feature to be enabled.
    pub fn metadata_by_summary_id(
        &self,
        summary_id: &SummaryId,
    ) -> Result<PackageMetadata<'_>, Error> {
        match &summary_id.source {
            SummarySource::Workspace { workspace_path } => {
                self.workspace().member_by_path(workspace_path)
            }
            SummarySource::Path { .. }
            | SummarySource::CratesIo
            | SummarySource::External { .. } => {
                // Do a linear search for now -- this appears to be the easiest thing to do and is
                // pretty fast. This could potentially be sped up by building an index by name, but
                // at least for reasonably-sized graphs it's really fast.
                //
                // TODO: consider optimizing this in the future.
                self.packages()
                    .find(|package| {
                        package.name() == summary_id.name
                            && package.version() == &summary_id.version
                            && package.source() == summary_id.source
                    })
                    .ok_or_else(|| Error::UnknownSummaryId(summary_id.clone()))
            }
        }
    }
}

impl PackageMetadata<'_> {
    /// Converts this metadata to a `SummaryId`.
    ///
    /// Requires the `summaries` feature to be enabled.
    pub fn to_summary_id(&self) -> SummaryId {
        SummaryId {
            name: self.name().to_string(),
            version: self.version().clone(),
            source: self.source().to_summary_source(),
        }
    }
}

/// A summary of the [`CargoSetInputs`] used to build a `CargoSet`.
///
/// Requires the `summaries` feature to be enabled.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub struct CargoSetInputsSummary {
    /// The Cargo resolver version used.
    ///
    /// For more information, see the documentation for [`CargoResolverVersion`].
    #[serde(alias = "version")]
    pub resolver: CargoResolverVersion,

    /// Whether dev-dependencies are included.
    pub include_dev: bool,

    /// The platform for which the initials are specified.
    #[serde(flatten)]
    pub initials_platform: InitialsPlatformSummary,

    /// The host platform.
    #[serde(default)]
    pub host_platform: PlatformSpecSummary,

    /// The target platform.
    #[serde(default)]
    pub target_platform: PlatformSpecSummary,

    /// The set of packages omitted from computations.
    #[serde(skip_serializing_if = "PackageSetSummary::is_empty", default)]
    pub omitted_packages: PackageSetSummary,

    /// The packages that formed the features-only set.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub features_only: Vec<FeaturesOnlySummary>,
}

impl CargoSetInputsSummary {
    /// Creates a new `CargoSetInputsSummary` from the given inputs.
    pub fn new(inputs: &CargoSetInputs<'_>) -> Result<Self, Error> {
        let graph = inputs.features_only.graph().package_graph;
        let features_only = &inputs.features_only;
        let opts = &inputs.options;
        let omitted_packages =
            PackageSetSummary::from_package_ids(graph, opts.omitted_packages.iter().copied())?;

        let mut features_only = features_only
            .packages_with_features(DependencyDirection::Forward)
            .map(|features| FeaturesOnlySummary {
                summary_id: features.package().to_summary_id(),
                base: features.has_base(),
                features: features
                    .named_features()
                    .map(|feature| feature.to_owned())
                    .collect(),
                optional_deps: features
                    .optional_deps()
                    .map(|feature| feature.to_owned())
                    .collect(),
            })
            .collect::<Vec<_>>();
        features_only.sort_unstable();

        Ok(Self {
            resolver: opts.resolver,
            include_dev: opts.include_dev,
            initials_platform: InitialsPlatformSummary::V2 {
                initials_platform: opts.initials_platform,
            },
            host_platform: PlatformSpecSummary::new(&opts.host_platform),
            target_platform: PlatformSpecSummary::new(&opts.target_platform),
            omitted_packages,
            features_only,
        })
    }

    /// Creates a new [`CargoSetInputs`] from this summary.
    ///
    /// Returns an error if any of the omitted packages, the platforms, or the
    /// features-only elements could not be resolved against `package_graph`.
    pub fn to_cargo_set_inputs<'g>(
        &'g self,
        package_graph: &'g PackageGraph,
    ) -> Result<CargoSetInputs<'g>, Error> {
        let omitted_packages = self
            .omitted_packages
            .to_package_set(package_graph, "resolving omitted-packages")?;

        let features_only = self.to_features_only(package_graph)?;

        let mut options = CargoOptions::new();
        options
            .set_resolver(self.resolver)
            .set_include_dev(self.include_dev)
            .set_initials_platform(self.initials_platform.into())
            .set_host_platform(self.host_platform.to_platform_spec().map_err(|error| {
                Error::InvalidPlatformSpecSummary {
                    build_platform: BuildPlatform::Host,
                    error,
                }
            })?)
            .set_target_platform(self.target_platform.to_platform_spec().map_err(|error| {
                Error::InvalidPlatformSpecSummary {
                    build_platform: BuildPlatform::Target,
                    error,
                }
            })?)
            .add_omitted_packages(omitted_packages.package_ids(DependencyDirection::Forward));
        Ok(CargoSetInputs {
            options,
            features_only,
        })
    }

    fn to_features_only<'g>(
        &'g self,
        package_graph: &'g PackageGraph,
    ) -> Result<FeatureSet<'g>, Error> {
        let feature_graph = package_graph.feature_graph();
        let mut feature_ids = Vec::new();
        let mut unknown_summary_ids = Vec::new();
        let mut unknown_features = Vec::new();
        let mut empty_entries = Vec::new();

        for features_only in &self.features_only {
            let metadata = match package_graph.metadata_by_summary_id(&features_only.summary_id) {
                Ok(metadata) => metadata,
                Err(Error::UnknownWorkspacePath(_) | Error::UnknownSummaryId(_)) => {
                    unknown_summary_ids.push(features_only.summary_id.clone());
                    continue;
                }
                Err(err) => return Err(err),
            };
            let package_id = metadata.id();

            if features_only.is_empty() {
                empty_entries.push(features_only.summary_id.clone());
                continue;
            }

            if features_only.base {
                feature_ids.push(FeatureId::base(package_id));
            }

            let mut unknown = UnknownFeatures {
                summary_id: features_only.summary_id.clone(),
                features: BTreeSet::new(),
                optional_deps: BTreeSet::new(),
            };
            for feature in &features_only.features {
                let feature_id = FeatureId::named(package_id, feature);
                if feature_graph.contains(feature_id) {
                    feature_ids.push(feature_id);
                } else {
                    unknown.features.insert(feature.clone());
                }
            }
            for dep_name in &features_only.optional_deps {
                let feature_id = FeatureId::optional_dependency(package_id, dep_name);
                if feature_graph.contains(feature_id) {
                    feature_ids.push(feature_id);
                } else {
                    unknown.optional_deps.insert(dep_name.clone());
                }
            }
            if !unknown.features.is_empty() || !unknown.optional_deps.is_empty() {
                unknown_features.push(unknown);
            }
        }

        if !unknown_summary_ids.is_empty()
            || !unknown_features.is_empty()
            || !empty_entries.is_empty()
        {
            return Err(Error::InvalidFeaturesOnlySummary {
                unknown_summary_ids,
                unknown_features,
                empty_entries,
            });
        }

        // Every ID above was checked against the feature graph, so a failure
        // here is a programmer error.
        Ok(feature_graph
            .resolve_ids(feature_ids)
            .expect("feature IDs were checked against the feature graph"))
    }
}

/// Summary information for `InitialsPlatform`.
#[derive(Copy, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged, rename_all = "kebab-case")]
#[non_exhaustive]
pub enum InitialsPlatformSummary {
    /// The first version of this option, which only allowed setting `proc-macros-on-target`.
    #[serde(rename_all = "kebab-case")]
    V1 {
        /// If set to true, this is treated as `InitialsPlatform::ProcMacrosOnTarget`, otherwise as
        /// `InitialsPlatform::Standard`.
        proc_macros_on_target: bool,
    },
    /// The second and current version of this option.
    #[serde(rename_all = "kebab-case")]
    V2 {
        /// The configuration value.
        initials_platform: InitialsPlatform,
    },
}

impl From<InitialsPlatformSummary> for InitialsPlatform {
    fn from(s: InitialsPlatformSummary) -> Self {
        match s {
            InitialsPlatformSummary::V1 {
                proc_macros_on_target,
            } => {
                if proc_macros_on_target {
                    InitialsPlatform::ProcMacrosOnTarget
                } else {
                    InitialsPlatform::Standard
                }
            }
            InitialsPlatformSummary::V2 { initials_platform } => initials_platform,
        }
    }
}

/// Summary information for a features-only package.
///
/// These packages are stored in `CargoSetInputsSummary` because they may or may not be in the final
/// build set.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub struct FeaturesOnlySummary {
    /// The summary ID for this package.
    #[serde(flatten)]
    pub summary_id: SummaryId,

    /// Whether the base feature is enabled for this package.
    ///
    /// Summaries written before this field existed do not record it, so it
    /// defaults to true.
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub base: bool,

    /// The named features built for this package.
    pub features: BTreeSet<String>,

    /// The optional dependencies built for this package.
    #[serde(skip_serializing_if = "BTreeSet::is_empty", default)]
    pub optional_deps: BTreeSet<String>,
}

impl FeaturesOnlySummary {
    /// Returns true if this entry is vacuous, which requires the following
    /// conditions to all be met:
    ///
    /// 1. `base` is false.
    /// 2. There are no named features.
    /// 3. There are no optional dependencies.
    ///
    /// [`CargoSetInputsSummary::new`] never produces such an entry, but a
    /// hand-edited summary can.
    ///
    /// [`CargoSetInputsSummary::to_cargo_set_inputs`] rejects cases for which
    /// this method returns true, since this is most likely an error.
    pub fn is_empty(&self) -> bool {
        !self.base && self.features.is_empty() && self.optional_deps.is_empty()
    }
}

fn default_true() -> bool {
    true
}

fn is_true(value: &bool) -> bool {
    *value
}

/// The features and optional dependencies of a features-only package that
/// were unknown to the `FeatureGraph`.
///
/// Returned as part of [`Error::InvalidFeaturesOnlySummary`].
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct UnknownFeatures {
    /// The summary ID for this package.
    pub summary_id: SummaryId,

    /// The named features that weren't known.
    pub features: BTreeSet<String>,

    /// The optional dependencies that weren't known.
    pub optional_deps: BTreeSet<String>,
}

impl PackageSource<'_> {
    /// Converts a `PackageSource` into a `SummarySource`.
    ///
    /// Requires the `summaries` feature to be enabled.
    pub fn to_summary_source(&self) -> SummarySource {
        match self {
            PackageSource::Workspace(path) => SummarySource::workspace(path),
            PackageSource::Path(path) => SummarySource::path(path),
            PackageSource::External(source) => {
                if *source == PackageSource::CRATES_IO_REGISTRY {
                    SummarySource::crates_io()
                } else {
                    SummarySource::external(*source)
                }
            }
        }
    }
}

impl PartialEq<SummarySource> for PackageSource<'_> {
    fn eq(&self, summary_source: &SummarySource) -> bool {
        match summary_source {
            SummarySource::Workspace { workspace_path } => {
                self == &PackageSource::Workspace(workspace_path)
            }
            SummarySource::Path { path } => self == &PackageSource::Path(path),
            SummarySource::CratesIo => {
                self == &PackageSource::External(PackageSource::CRATES_IO_REGISTRY)
            }
            SummarySource::External { source } => self == &PackageSource::External(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_old_metadata() {
        // Ensure that previous versions of the metadata parse correctly.
        // TODO: note that there have been some compatibility breaks, particularly for
        // omitted-packages. Probably don't need to retain too much backwards compatibility.
        let metadata = "\
version = 'v1'
include-dev = true
proc-macros-on-target = false
";

        let summary: CargoSetInputsSummary = toml::from_str(metadata).expect("parsed correctly");
        assert_eq!(
            InitialsPlatform::from(summary.initials_platform),
            InitialsPlatform::Standard
        );
    }

    #[test]
    fn parse_features_only_metadata() {
        let metadata = "\
resolver = '2'
include-dev = false
initials-platform = 'standard'

[[features-only]]
name = 'guppy'
version = '0.5.0'
workspace-path = 'guppy'
features = ['guppy-summaries', 'summaries']
optional-deps = ['guppy-summaries']
";

        let summary: CargoSetInputsSummary = toml::from_str(metadata).expect("parsed correctly");
        let features_only = match summary.features_only.as_slice() {
            [features_only] => features_only,
            other => panic!("expected exactly one features-only entry, found {other:?}"),
        };
        assert_eq!(features_only.summary_id.name, "guppy");
        assert_eq!(
            features_only.summary_id.source,
            SummarySource::workspace("guppy")
        );
        assert!(features_only.base, "base defaults to true when absent");
        assert_eq!(
            features_only.features.iter().collect::<Vec<_>>(),
            ["guppy-summaries", "summaries"]
        );
        assert_eq!(
            features_only.optional_deps.iter().collect::<Vec<_>>(),
            ["guppy-summaries"]
        );
    }

    #[test]
    fn parse_features_only_without_base() {
        let metadata = "\
resolver = '2'
include-dev = false
initials-platform = 'standard'

[[features-only]]
name = 'guppy'
version = '0.5.0'
workspace-path = 'guppy'
base = false
features = ['summaries']
";

        let summary: CargoSetInputsSummary = toml::from_str(metadata).expect("parsed correctly");
        let features_only = match summary.features_only.as_slice() {
            [features_only] => features_only,
            other => panic!("expected exactly one features-only entry, found {other:?}"),
        };
        assert!(!features_only.base, "base parsed as false");

        let serialized = toml::to_string(&summary).expect("serialized correctly");
        assert!(
            serialized.contains("base = false"),
            "base = false is written out: {serialized}"
        );
        let reparsed: CargoSetInputsSummary =
            toml::from_str(&serialized).expect("reparsed correctly");
        assert_eq!(reparsed, summary, "summary round-tripped through TOML");
    }
}
