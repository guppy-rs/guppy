// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use fixtures::{json::JsonFixture, package_id};
use guppy::{
    PackageId,
    graph::{
        DependencyDirection, PackageGraph,
        cargo::{CargoOptions, CargoResolverVersion, CargoSet},
        feature::{
            ConditionalLink, FeatureId, FeatureLabel, FeatureSet, LinkDeclarations,
            StandardFeatures, feature_id_filter,
        },
    },
    platform::PlatformStatus,
};
use std::{collections::BTreeSet, iter};
use target_spec::{Platform, TargetFeatures};

pub(super) const LINUX: &str = "x86_64-unknown-linux-gnu";
pub(super) const WINDOWS: &str = "x86_64-pc-windows-msvc";
pub(super) const MACOS: &str = "aarch64-apple-darwin";

pub(super) struct CargoResolutionCase {
    /// The Cargo feature resolver version -- v2 by default.
    resolver: CargoResolverVersion,

    /// The target triple -- Linux by default.
    target_platform: &'static str,

    /// The host triple -- Linux by default.
    host_platform: &'static str,

    /// Whether dev-dependencies of the initial package are built, as with
    /// `cargo build --tests` -- off by default.
    include_dev: bool,

    /// Features enabled on the initial package.
    ///
    /// `dep:x` features are accepted as valid.
    features: &'static [&'static str],

    /// Expected `(package, features)` pairs in the target feature set.
    ///
    /// Features are written in Cargo syntax and are space separated:
    ///
    /// * A bare name is a named feature.
    /// * `dep:x` is an optional dependency.
    /// * The base is implicit.
    ///
    /// If the features (second element of the pair) is `None`, the package is
    /// not expected to be built on that platform at all.
    target_expected: &'static [ExpectedFeatures],

    /// Expected `(package, features)` pairs in the host feature set.
    host_expected: &'static [ExpectedFeatures],
}

pub(super) type ExpectedFeatures = (&'static str, Option<&'static str>);

impl CargoResolutionCase {
    pub(super) const fn new(features: &'static [&'static str]) -> Self {
        Self {
            resolver: CargoResolverVersion::V2,
            target_platform: LINUX,
            host_platform: LINUX,
            include_dev: false,
            features,
            target_expected: &[],
            host_expected: &[],
        }
    }

    pub(super) const fn resolver(self, resolver: CargoResolverVersion) -> Self {
        Self { resolver, ..self }
    }

    pub(super) const fn target_platform(self, target_platform: &'static str) -> Self {
        Self {
            target_platform,
            ..self
        }
    }

    pub(super) const fn host_platform(self, host_platform: &'static str) -> Self {
        Self {
            host_platform,
            ..self
        }
    }

    pub(super) const fn include_dev(self) -> Self {
        Self {
            include_dev: true,
            ..self
        }
    }

    pub(super) const fn target_expected(
        self,
        target_expected: &'static [ExpectedFeatures],
    ) -> Self {
        Self {
            target_expected,
            ..self
        }
    }

    pub(super) const fn host_expected(self, host_expected: &'static [ExpectedFeatures]) -> Self {
        Self {
            host_expected,
            ..self
        }
    }

    pub(super) fn check(&self, graph: &PackageGraph, initial: &str, msg_prefix: &str) {
        let cargo_set = self.cargo_set(graph, initial);
        let msg = format!(
            "{msg_prefix}while checking {:?} resolution of {} on target {} and host {} \
             (include dev: {}) for {}",
            self.resolver,
            initial,
            self.target_platform,
            self.host_platform,
            self.include_dev,
            self.features.join(" ")
        );
        for (id, expected) in self.target_expected {
            assert_features_for_package(
                cargo_set.target_features(),
                &package_id(*id),
                *expected,
                &format!("{msg} (target)"),
            );
        }
        for (id, expected) in self.host_expected {
            assert_features_for_package(
                cargo_set.host_features(),
                &package_id(*id),
                *expected,
                &format!("{msg} (host)"),
            );
        }
    }

    fn cargo_set<'g>(&self, graph: &'g PackageGraph, initial: &str) -> CargoSet<'g> {
        let initial = package_id(initial);
        let features = self.features.join(" ");
        let feature_set = graph
            .resolve_ids([&initial])
            .expect("valid package ID")
            .to_feature_set(feature_id_filter(
                StandardFeatures::Default,
                feature_ids(&initial, &features),
            ));

        let mut cargo_options = CargoOptions::new();
        cargo_options
            .set_resolver(self.resolver)
            .set_include_dev(self.include_dev)
            .set_target_platform(platform(self.target_platform))
            .set_host_platform(platform(self.host_platform));
        feature_set
            .into_cargo_set(&cargo_options)
            .expect("resolving cargo should work")
    }
}

fn platform(triple: &'static str) -> Platform {
    Platform::new(triple, TargetFeatures::Unknown).expect("valid triple")
}

pub(super) fn assert_features_for_package(
    feature_set: &FeatureSet<'_>,
    package_id: &PackageId,
    expected: Option<&str>,
    msg: &str,
) {
    let actual = feature_set
        .features_for(package_id)
        .expect("valid package ID");
    let expected = expected.map(feature_labels);

    assert_eq!(
        actual.as_ref().map(|list| list.labels()),
        expected.as_deref(),
        "{msg}: for package {package_id}, features in feature set match"
    );
}

/// Parses a space-separated feature list in Cargo's syntax
/// into a list of `FeatureLabel` instances.
///
/// * A bare name is a named feature.
/// * `dep:x` is an optional dependency.
/// * The base feature is implicit.
///
/// The list is returned in sorted order and is deduplicated.
pub(super) fn feature_labels(features: &str) -> Vec<FeatureLabel<'_>> {
    let mut labels: Vec<_> = iter::once(FeatureLabel::Base)
        .chain(
            features
                .split_whitespace()
                .map(|feature| match feature.strip_prefix("dep:") {
                    Some(dep_name) => FeatureLabel::OptionalDependency(dep_name),
                    None => FeatureLabel::Named(feature),
                }),
        )
        .collect();
    labels.sort_unstable();
    labels.dedup();
    labels
}

pub(super) fn feature_ids<'a>(
    package_id: &'a PackageId,
    features: &'a str,
) -> impl Iterator<Item = FeatureId<'a>> {
    feature_labels(features)
        .into_iter()
        .map(move |label| FeatureId::new(package_id, label))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum VisitStatus {
    Always,
    Never,
    // Enabled on platforms matching these target specs.
    Specs(BTreeSet<String>),
}

impl VisitStatus {
    pub(super) fn new(status: PlatformStatus<'_>) -> Self {
        match status {
            PlatformStatus::Always => VisitStatus::Always,
            PlatformStatus::Never => VisitStatus::Never,
            PlatformStatus::PlatformDependent { eval } => VisitStatus::Specs(
                eval.target_specs()
                    .iter()
                    .map(|spec| spec.to_string())
                    .collect(),
            ),
        }
    }
}

pub(super) fn specs(specs: &[&str]) -> VisitStatus {
    VisitStatus::Specs(specs.iter().map(|spec| (*spec).to_owned()).collect())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SeenLink {
    pub(super) declarations: LinkDeclarations,
    pub(super) normal: VisitStatus,
    pub(super) build: VisitStatus,
    pub(super) dev: VisitStatus,
}

impl SeenLink {
    pub(super) fn from_link(link: &ConditionalLink<'_>) -> Self {
        Self {
            declarations: link.declarations(),
            normal: VisitStatus::new(link.normal()),
            build: VisitStatus::new(link.build()),
            dev: VisitStatus::new(link.dev()),
        }
    }
}

pub(super) fn graph_with_patched_json(
    fixture: &JsonFixture,
    patch: impl FnOnce(&mut serde_json::Value),
) -> PackageGraph {
    let mut metadata: serde_json::Value =
        serde_json::from_str(fixture.json()).expect("fixture is valid JSON");
    patch(&mut metadata);
    PackageGraph::from_json(metadata.to_string()).expect("patched metadata is valid")
}

pub(super) fn conditional_links_from<'g>(
    graph: &'g PackageGraph,
    from: FeatureId<'_>,
) -> Vec<ConditionalLink<'g>> {
    graph
        .feature_graph()
        .query_forward([from])
        .expect("valid feature ID")
        .resolve()
        .conditional_links(DependencyDirection::Forward)
        .filter(|link| link.from().feature_id() == from)
        .collect()
}
