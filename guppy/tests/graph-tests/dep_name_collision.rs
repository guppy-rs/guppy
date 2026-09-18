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

use crate::feature_helpers::{CargoResolutionCase, WINDOWS};
use fixtures::{
    json::{self, JsonFixture},
    package_id,
};
use guppy::graph::{DependencyDirection, feature::FeatureId};

#[rustfmt::skip]
static CASES: &[CargoResolutionCase] = &[
    // `renamed` -> bytes (always) + bitflags (`cfg(any())`, so never built).
    CargoResolutionCase::new(&["dep-colon"]).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,     Some("default dep-colon dep:renamed")),
        (json::METADATA_DEP_NAME_COLLISION_BYTES,    Some("")),
        (json::METADATA_DEP_NAME_COLLISION_BITFLAGS, None),
    ]),
    CargoResolutionCase::new(&["slash"]).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,     Some("default slash dep:renamed")),
        (json::METADATA_DEP_NAME_COLLISION_BYTES,    Some("std")),
        (json::METADATA_DEP_NAME_COLLISION_BITFLAGS, None),
    ]),
    // A weak `renamed?/std` on its own activates nothing.
    CargoResolutionCase::new(&["weak-slash"]).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,     Some("default weak-slash")),
        (json::METADATA_DEP_NAME_COLLISION_BYTES,    None),
        (json::METADATA_DEP_NAME_COLLISION_BITFLAGS, None),
    ]),
    // ... but once `dep:renamed` activates it, the buffered weak edge flushes.
    CargoResolutionCase::new(&["weak-slash", "dep-colon"]).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,     Some("default dep-colon weak-slash dep:renamed")),
        (json::METADATA_DEP_NAME_COLLISION_BYTES,    Some("std")),
        (json::METADATA_DEP_NAME_COLLISION_BITFLAGS, None),
    ]),

    // `split` -> arrayvec on Linux, tinyvec on Windows. The two platform
    // conditions are unioned, so the name activates on both -- pulling in only
    // the package that is buildable there.
    CargoResolutionCase::new(&["split-dep-colon"]).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,     Some("default split-dep-colon dep:split")),
        (json::METADATA_DEP_NAME_COLLISION_ARRAYVEC, Some("")),
        (json::METADATA_DEP_NAME_COLLISION_TINYVEC,  None),
    ]),
    CargoResolutionCase::new(&["split-dep-colon"]).target_platform(WINDOWS).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,     Some("default split-dep-colon dep:split")),
        (json::METADATA_DEP_NAME_COLLISION_ARRAYVEC, None),
        (json::METADATA_DEP_NAME_COLLISION_TINYVEC,  Some("")),
    ]),
    CargoResolutionCase::new(&["split-slash"]).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,     Some("default split-slash dep:split")),
        (json::METADATA_DEP_NAME_COLLISION_ARRAYVEC, Some("std")),
        (json::METADATA_DEP_NAME_COLLISION_TINYVEC,  None),
    ]),
    CargoResolutionCase::new(&["split-slash"]).target_platform(WINDOWS).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,     Some("default split-slash dep:split")),
        (json::METADATA_DEP_NAME_COLLISION_ARRAYVEC, None),
        (json::METADATA_DEP_NAME_COLLISION_TINYVEC,  Some("alloc std tinyvec_macros dep:tinyvec_macros")),
    ]),

    // `both` -> libc and (on unix) memchr, so on Linux both links are live at
    // once. A weak `both?/std` buffers one weak index per link, and activating
    // `dep:both` has to flush both.
    CargoResolutionCase::new(&["both-weak-slash"]).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_LIBC,     None),
        (json::METADATA_DEP_NAME_COLLISION_MEMCHR,   None),
    ]),
    CargoResolutionCase::new(&["both-weak-slash", "both-dep-colon"]).target_expected(&[
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

/// `ConditionalLink::package_links` reports every package a dependency name
/// resolves to.
///
/// `dep:renamed` activates the name itself, so its link is derived from both
/// `main -> bytes` and `main -> bitflags`. A `renamed/std` cross-package link
/// is derived from just the package it lands on.
#[test]
fn package_links_report_every_package() {
    let graph = JsonFixture::metadata_dep_name_collision().graph();
    let main = package_id(json::METADATA_DEP_NAME_COLLISION_MAIN);
    let bytes = package_id(json::METADATA_DEP_NAME_COLLISION_BYTES);
    let bitflags = package_id(json::METADATA_DEP_NAME_COLLISION_BITFLAGS);

    let links_out_of = |feature| {
        let from = FeatureId::named(&main, feature);
        let mut links: Vec<_> = graph
            .feature_graph()
            .query_forward([from])
            .expect("valid feature")
            .resolve()
            .conditional_links(DependencyDirection::Forward)
            .filter(|link| link.from().feature_id() == from)
            .map(|link| {
                let mut to_names: Vec<_> = link
                    .package_links()
                    .map(|package_link| package_link.to().name())
                    .collect();
                to_names.sort();
                (link.to().feature_id(), to_names)
            })
            .collect();
        links.sort();
        links
    };

    assert_eq!(
        links_out_of("dep-colon"),
        [(
            FeatureId::optional_dependency(&main, "renamed"),
            vec!["bitflags", "bytes"],
        )],
        "dep:renamed is derived from both links"
    );

    assert_eq!(
        links_out_of("slash"),
        [
            (
                FeatureId::optional_dependency(&main, "renamed"),
                vec!["bitflags", "bytes"],
            ),
            (FeatureId::named(&bitflags, "std"), vec!["bitflags"]),
            (FeatureId::named(&bytes, "std"), vec!["bytes"]),
        ],
        "each cross-package link is derived from one link, dep:renamed from both"
    );
}

#[test]
fn resolution_matches_cargo() {
    let graph = JsonFixture::metadata_dep_name_collision().graph();
    for case in CASES {
        case.check(graph, json::METADATA_DEP_NAME_COLLISION_MAIN, "");
    }
}
