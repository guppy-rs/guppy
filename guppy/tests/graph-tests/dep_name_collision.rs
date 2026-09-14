// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Tests for the case where one dependency name maps to two different packages.
//!
//! See `add_named_feature_edges` in `guppy/src/graph/feature/build.rs` for how a
//! `package = "..."` rename produces this, and why every `dep:name` and
//! `name/feature` entry has to be resolved against all of the links.
//!
//! The `dep-name-collision` fixture's `main` package declares three such names,
//! one per interesting platform combination:
//!
//! ```toml
//! [features]
//! dep-colon = ["dep:renamed"]
//! slash = ["renamed/std"]
//! weak-slash = ["renamed?/std"]
//! split-dep-colon = ["dep:split"]
//! split-slash = ["split/std"]
//! both-dep-colon = ["dep:both"]
//! both-weak-slash = ["both?/std"]
//!
//! # One link everywhere, one link never: the shape semver 1.0.28 uses to keep
//! # a package in the lockfile without ever building it.
//! [dependencies.renamed]
//! version = "1"
//! package = "bytes"
//! optional = true
//! default-features = false
//!
//! [target.'cfg(any())'.dependencies.renamed]
//! version = "2"
//! package = "bitflags"
//! optional = true
//! default-features = false
//!
//! # One link per platform: the two platform conditions have to be unioned.
//! [target.'cfg(target_os = "linux")'.dependencies.split]
//! version = "0.7"
//! package = "arrayvec"
//! optional = true
//! default-features = false
//!
//! [target.'cfg(windows)'.dependencies.split]
//! version = "1"
//! package = "tinyvec"
//! optional = true
//! default-features = false
//!
//! # Both links live at once on Linux.
//! [dependencies.both]
//! version = "0.2"
//! package = "libc"
//! optional = true
//! default-features = false
//!
//! [target.'cfg(unix)'.dependencies.both]
//! version = "2"
//! package = "memchr"
//! optional = true
//! default-features = false
//! ```

use crate::feature_helpers::assert_features_for_package;
use fixtures::{
    json::{self, JsonFixture},
    package_id,
};
use guppy::graph::{
    cargo::{CargoOptions, CargoResolverVersion},
    feature::{FeatureLabel, StandardFeatures, named_feature_filter},
};
use std::iter;
use target_spec::{Platform, TargetFeatures};

const LINUX: &str = "x86_64-unknown-linux-gnu";
const WINDOWS: &str = "x86_64-pc-windows-msvc";

/// Resolution cases: target triple, features enabled on `main`, and the
/// expected result as `(package, features)` pairs.
///
/// Features are written in Cargo's own syntax, space-separated: a bare name is
/// a named feature, `dep:x` an optional dependency, and the base is implicit.
/// `None` means the package is not built on that platform at all.
type Case = (&'static str, &'static str, &'static [Expected]);
type Expected = (&'static str, Option<&'static str>);

#[rustfmt::skip]
static CASES: &[Case] = &[
    // `renamed` -> bytes (always) + bitflags (`cfg(any())`, so never built).
    (LINUX, "dep-colon", &[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,     Some("default dep-colon dep:renamed")),
        (json::METADATA_DEP_NAME_COLLISION_BYTES,    Some("")),
        (json::METADATA_DEP_NAME_COLLISION_BITFLAGS, None),
    ]),
    (LINUX, "slash", &[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,     Some("default slash dep:renamed")),
        (json::METADATA_DEP_NAME_COLLISION_BYTES,    Some("std")),
        (json::METADATA_DEP_NAME_COLLISION_BITFLAGS, None),
    ]),
    // A weak `renamed?/std` on its own activates nothing.
    (LINUX, "weak-slash", &[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,     Some("default weak-slash")),
        (json::METADATA_DEP_NAME_COLLISION_BYTES,    None),
        (json::METADATA_DEP_NAME_COLLISION_BITFLAGS, None),
    ]),
    // ... but once `dep:renamed` activates it, the buffered weak edge flushes.
    (LINUX, "weak-slash dep-colon", &[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,     Some("default dep-colon weak-slash dep:renamed")),
        (json::METADATA_DEP_NAME_COLLISION_BYTES,    Some("std")),
        (json::METADATA_DEP_NAME_COLLISION_BITFLAGS, None),
    ]),

    // `split` -> arrayvec on Linux, tinyvec on Windows. The two platform
    // conditions are unioned, so the name activates on both -- pulling in only
    // the package that is buildable there.
    (LINUX, "split-dep-colon", &[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,     Some("default split-dep-colon dep:split")),
        (json::METADATA_DEP_NAME_COLLISION_ARRAYVEC, Some("")),
        (json::METADATA_DEP_NAME_COLLISION_TINYVEC,  None),
    ]),
    (WINDOWS, "split-dep-colon", &[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,     Some("default split-dep-colon dep:split")),
        (json::METADATA_DEP_NAME_COLLISION_ARRAYVEC, None),
        (json::METADATA_DEP_NAME_COLLISION_TINYVEC,  Some("")),
    ]),
    (LINUX, "split-slash", &[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,     Some("default split-slash dep:split")),
        (json::METADATA_DEP_NAME_COLLISION_ARRAYVEC, Some("std")),
        (json::METADATA_DEP_NAME_COLLISION_TINYVEC,  None),
    ]),
    (WINDOWS, "split-slash", &[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,     Some("default split-slash dep:split")),
        (json::METADATA_DEP_NAME_COLLISION_ARRAYVEC, None),
        (json::METADATA_DEP_NAME_COLLISION_TINYVEC,  Some("alloc std tinyvec_macros dep:tinyvec_macros")),
    ]),

    // `both` -> libc and (on unix) memchr, so on Linux both links are live at
    // once. A weak `both?/std` buffers one weak index per link, and activating
    // `dep:both` has to flush both -- the merged `dep:` edge only carries one
    // of the two `package_edge_ix`es.
    (LINUX, "both-weak-slash", &[
        (json::METADATA_DEP_NAME_COLLISION_LIBC,     None),
        (json::METADATA_DEP_NAME_COLLISION_MEMCHR,   None),
    ]),
    (LINUX, "both-weak-slash both-dep-colon", &[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,     Some("both-dep-colon both-weak-slash default dep:both")),
        (json::METADATA_DEP_NAME_COLLISION_LIBC,     Some("std")),
        // memchr's `std` feature implies `alloc`.
        (json::METADATA_DEP_NAME_COLLISION_MEMCHR,   Some("alloc std")),
    ]),
];

/// Both links share the dependency name, so both must be in the package graph.
#[test]
fn both_links_share_a_dep_name() {
    let graph = JsonFixture::metadata_dep_name_collision().graph();
    let main = graph
        .metadata(&package_id(json::METADATA_DEP_NAME_COLLISION_MAIN))
        .expect("valid package ID");

    let mut links: Vec<_> = main
        .direct_links()
        .map(|link| (link.dep_name(), link.to().name()))
        .collect();
    links.sort();

    assert_eq!(
        links,
        [
            ("both", "libc"),
            ("both", "memchr"),
            ("renamed", "bitflags"),
            ("renamed", "bytes"),
            ("split", "arrayvec"),
            ("split", "tinyvec"),
        ],
        "both links for each dep name are present"
    );
}

#[test]
fn resolution_matches_cargo() {
    for (triple, features, expected) in CASES {
        let feature_set = JsonFixture::metadata_dep_name_collision()
            .graph()
            .resolve_ids([&package_id(json::METADATA_DEP_NAME_COLLISION_MAIN)])
            .expect("valid package ID")
            .to_feature_set(named_feature_filter(
                StandardFeatures::Default,
                features.split_whitespace(),
            ));

        let mut cargo_options = CargoOptions::new();
        cargo_options
            .set_resolver(CargoResolverVersion::V2)
            .set_target_platform(Platform::new(*triple, TargetFeatures::Unknown).unwrap());
        let cargo_set = feature_set
            .into_cargo_set(&cargo_options)
            .expect("resolving cargo should work");

        let msg = format!("while checking {triple} resolution for default + {features}");
        for (id, expected) in *expected {
            let expected = expected.map(parse_labels);
            assert_features_for_package(
                cargo_set.target_features(),
                &package_id(*id),
                expected.as_deref(),
                &msg,
            );
        }
    }
}

/// Parses a space-separated feature list written in Cargo's syntax into the
/// labels `FeatureList` reports, which are ordered base, named, optional
/// dependency.
fn parse_labels(features: &str) -> Vec<FeatureLabel<'_>> {
    iter::once(FeatureLabel::Base)
        .chain(
            features
                .split_whitespace()
                .map(|feature| match feature.strip_prefix("dep:") {
                    Some(dep_name) => FeatureLabel::OptionalDependency(dep_name),
                    None => FeatureLabel::Named(feature),
                }),
        )
        .collect()
}
