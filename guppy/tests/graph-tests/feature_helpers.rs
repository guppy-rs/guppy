// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use guppy::{
    PackageId,
    graph::feature::{FeatureLabel, FeatureSet},
};
use std::iter;

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
