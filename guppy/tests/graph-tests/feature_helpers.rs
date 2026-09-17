// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use fixtures::package_id;
use guppy::{
    PackageId,
    graph::{
        PackageGraph,
        cargo::{CargoOptions, CargoResolverVersion, CargoSet},
        feature::{FeatureLabel, FeatureSet, StandardFeatures, named_feature_filter},
    },
};
use std::iter;
use target_spec::{Platform, TargetFeatures};

pub(super) const LINUX: &str = "x86_64-unknown-linux-gnu";
pub(super) const WINDOWS: &str = "x86_64-pc-windows-msvc";

pub(super) struct CargoResolutionCase {
    /// The Cargo feature resolver version -- v2 by default.
    resolver: CargoResolverVersion,

    /// The host triple -- Linux by default. The target is always Linux.
    host_platform: &'static str,

    /// Features enabled on the initial package.
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
            host_platform: LINUX,
            features,
            target_expected: &[],
            host_expected: &[],
        }
    }

    pub(super) const fn resolver(self, resolver: CargoResolverVersion) -> Self {
        Self { resolver, ..self }
    }

    pub(super) const fn host_platform(self, host_platform: &'static str) -> Self {
        Self {
            host_platform,
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
            "{msg_prefix}while checking {:?} resolution of {} on a {} host for {}",
            self.resolver,
            initial,
            self.host_platform,
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
        let feature_set = graph
            .resolve_ids([&package_id(initial)])
            .expect("valid package ID")
            .to_feature_set(named_feature_filter(
                StandardFeatures::Default,
                self.features.iter().copied(),
            ));

        let mut cargo_options = CargoOptions::new();
        cargo_options
            .set_resolver(self.resolver)
            .set_target_platform(platform(LINUX))
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
