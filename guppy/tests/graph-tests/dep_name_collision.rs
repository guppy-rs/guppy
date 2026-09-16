// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Tests for the case where one dependency name maps to two different packages.
//!
//! See `add_named_feature_edges` in `guppy/src/graph/feature/build.rs` for how a
//! `package = "..."` rename produces this, and why every `dep:name` and
//! `name/feature` entry has to be resolved against all of the links.
//!
//! The `dep-name-collision` fixture's `main` package declares five such names,
//! one per interesting platform combination:
//!
//! ```toml
//! [package]
//! build = "build.rs"
//!
//! [features]
//! default = []
//! dep-colon = ["dep:renamed"]
//! slash = ["renamed/std"]
//! weak-slash = ["renamed?/std"]
//! split-dep-colon = ["dep:split"]
//! split-slash = ["split/std"]
//! both-dep-colon = ["dep:both"]
//! both-weak-slash = ["both?/std"]
//! plain-slash = ["plain/std"]
//! kinds-dep-colon = ["dep:kinds"]
//! kinds-slash = ["kinds/std"]
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
//!
//! [dependencies.plain]
//! version = "1"
//! package = "either"
//! default-features = false
//!
//! [target.'cfg(any())'.dependencies.plain]
//! version = "1"
//! package = "byteorder"
//! default-features = false
//!
//! [dependencies.kinds]
//! version = "0.4"
//! package = "log"
//! optional = true
//! default-features = false
//!
//! [build-dependencies.kinds]
//! version = "0.4"
//! package = "hex"
//! optional = true
//! default-features = false
//! ```

use crate::feature_helpers::{CargoResolutionCase, WINDOWS};
use fixtures::{
    json::{self, JsonFixture},
    package_id,
};
use guppy::graph::{DependencyDirection, PackageGraph, feature::FeatureId};

#[rustfmt::skip]
static CASES: &[CargoResolutionCase] = &[
    // [features]
    // dep-colon = ["dep:renamed"]
    //
    // `renamed` resolves to two packages:
    //
    // * bytes as a plain (non-target) dependency.
    // * bitflags, declared under `cfg(any())`, so never built.
    CargoResolutionCase::new(&["dep-colon"]).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,     Some("default dep-colon dep:renamed")),
        (json::METADATA_DEP_NAME_COLLISION_BYTES,    Some("")),
        (json::METADATA_DEP_NAME_COLLISION_BITFLAGS, None),
    ]),
    // [features]
    // slash = ["renamed/std"]
    //
    // `renamed` resolves to bytes and bitflags as above.
    CargoResolutionCase::new(&["slash"]).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,     Some("default slash dep:renamed")),
        (json::METADATA_DEP_NAME_COLLISION_BYTES,    Some("std")),
        (json::METADATA_DEP_NAME_COLLISION_BITFLAGS, None),
    ]),
    // [features]
    // weak-slash = ["renamed?/std"]
    //
    // A weak `renamed?/std` on its own activates nothing.
    CargoResolutionCase::new(&["weak-slash"]).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,     Some("default weak-slash")),
        (json::METADATA_DEP_NAME_COLLISION_BYTES,    None),
        (json::METADATA_DEP_NAME_COLLISION_BITFLAGS, None),
    ]),
    // [features]
    // weak-slash = ["renamed?/std"]
    // dep-colon = ["dep:renamed"]
    //
    // Once `dep:renamed` activates the dependency, the buffered weak edge
    // flushes and bytes gets `std`.
    CargoResolutionCase::new(&["weak-slash", "dep-colon"]).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,     Some("default dep-colon weak-slash dep:renamed")),
        (json::METADATA_DEP_NAME_COLLISION_BYTES,    Some("std")),
        (json::METADATA_DEP_NAME_COLLISION_BITFLAGS, None),
    ]),

    // [features]
    // split-dep-colon = ["dep:split"]
    //
    // `split` resolves to two packages:
    //
    // * arrayvec, declared under `cfg(target_os = "linux")`.
    // * tinyvec, declared under `cfg(windows)`.
    //
    // The two conditions are unioned, so `dep:split` activates on both
    // platforms, pulling in only the package that is buildable there.
    CargoResolutionCase::new(&["split-dep-colon"]).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,     Some("default split-dep-colon dep:split")),
        (json::METADATA_DEP_NAME_COLLISION_ARRAYVEC, Some("")),
        (json::METADATA_DEP_NAME_COLLISION_TINYVEC,  None),
    ]),
    // The same on Windows.
    CargoResolutionCase::new(&["split-dep-colon"]).target_platform(WINDOWS).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,     Some("default split-dep-colon dep:split")),
        (json::METADATA_DEP_NAME_COLLISION_ARRAYVEC, None),
        (json::METADATA_DEP_NAME_COLLISION_TINYVEC,  Some("")),
    ]),
    // [features]
    // split-slash = ["split/std"]
    //
    // `split` resolves to arrayvec and tinyvec as above.
    CargoResolutionCase::new(&["split-slash"]).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,     Some("default split-slash dep:split")),
        (json::METADATA_DEP_NAME_COLLISION_ARRAYVEC, Some("std")),
        (json::METADATA_DEP_NAME_COLLISION_TINYVEC,  None),
    ]),
    // The same on Windows. tinyvec's `std` implies `alloc`, which implies its
    // optional `tinyvec_macros` dependency.
    CargoResolutionCase::new(&["split-slash"]).target_platform(WINDOWS).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,     Some("default split-slash dep:split")),
        (json::METADATA_DEP_NAME_COLLISION_ARRAYVEC, None),
        (json::METADATA_DEP_NAME_COLLISION_TINYVEC,  Some("alloc std tinyvec_macros dep:tinyvec_macros")),
    ]),

    // [features]
    // both-weak-slash = ["both?/std"]
    //
    // `both` resolves to two packages:
    //
    // * libc as a plain (non-target) dependency.
    // * memchr, declared under `cfg(unix)`.
    //
    // On Linux both are live at once. A weak `both?/std` buffers one weak
    // index per link and, on its own, activates nothing.
    CargoResolutionCase::new(&["both-weak-slash"]).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,     Some("both-weak-slash default")),
        (json::METADATA_DEP_NAME_COLLISION_LIBC,     None),
        (json::METADATA_DEP_NAME_COLLISION_MEMCHR,   None),
    ]),
    // [features]
    // both-weak-slash = ["both?/std"]
    // both-dep-colon = ["dep:both"]
    //
    // Activating `dep:both` has to flush both weak buffers. memchr's `std`
    // implies `alloc`.
    CargoResolutionCase::new(&["both-weak-slash", "both-dep-colon"]).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,     Some("both-dep-colon both-weak-slash default dep:both")),
        (json::METADATA_DEP_NAME_COLLISION_LIBC,     Some("std")),
        (json::METADATA_DEP_NAME_COLLISION_MEMCHR,   Some("alloc std")),
    ]),
    // The same on Windows, where only libc is live. Activating `dep:both`
    // releases memchr's weak buffer too, but memchr's link is `cfg(unix)`, so
    // `memchr/std` stays off.
    CargoResolutionCase::new(&["both-weak-slash", "both-dep-colon"]).target_platform(WINDOWS).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,     Some("both-dep-colon both-weak-slash default dep:both")),
        (json::METADATA_DEP_NAME_COLLISION_LIBC,     Some("std")),
        (json::METADATA_DEP_NAME_COLLISION_MEMCHR,   None),
    ]),

    // [dependencies]
    // plain = { package = "either" }
    //
    // [target.'cfg(any())'.dependencies]
    // plain = { package = "byteorder" }
    //
    // Neither declaration is optional, so there is no `dep:plain` feature and
    // either is always built, even with no features enabled.
    CargoResolutionCase::new(&[]).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,      Some("default")),
        (json::METADATA_DEP_NAME_COLLISION_EITHER,    Some("")),
        (json::METADATA_DEP_NAME_COLLISION_BYTEORDER, None),
    ]),
    // [features]
    // plain-slash = ["plain/std"]
    //
    // `plain` resolves to either and byteorder as above.
    CargoResolutionCase::new(&["plain-slash"]).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,      Some("default plain-slash")),
        (json::METADATA_DEP_NAME_COLLISION_EITHER,    Some("std")),
        (json::METADATA_DEP_NAME_COLLISION_BYTEORDER, None),
    ]),

    // [dependencies]
    // kinds = { package = "log", optional = true }
    //
    // [build-dependencies]
    // kinds = { package = "hex", optional = true }
    //
    // [features]
    // kinds-dep-colon = ["dep:kinds"]
    //
    // `main` has a build script, so `dep:kinds` activates log on the target
    // and hex on the host.
    CargoResolutionCase::new(&["kinds-dep-colon"]).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,      Some("default kinds-dep-colon dep:kinds")),
        (json::METADATA_DEP_NAME_COLLISION_LOG,       Some("")),
        (json::METADATA_DEP_NAME_COLLISION_HEX,       None),
    ]).host_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_HEX,       Some("")),
        (json::METADATA_DEP_NAME_COLLISION_LOG,       None),
    ]),
    // [features]
    // kinds-slash = ["kinds/std"]
    //
    // `kinds` resolves to log and hex as above. Both `std` features imply
    // `alloc`.
    CargoResolutionCase::new(&["kinds-slash"]).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,      Some("default kinds-slash dep:kinds")),
        (json::METADATA_DEP_NAME_COLLISION_LOG,       Some("alloc std")),
        (json::METADATA_DEP_NAME_COLLISION_HEX,       None),
    ]).host_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_HEX,       Some("alloc std")),
        (json::METADATA_DEP_NAME_COLLISION_LOG,       None),
    ]),
];

/// Test that `ConditionalLink::package_links` reports every package a
/// dependency name resolves to.
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

// Merged edges use whichever link `direct_links` yields first, so results must
// not depend on `resolve.nodes[].deps` order (which is the PackageGraph link
// insertion order internally).
#[test]
fn resolution_independent_of_link_order() {
    let fixture = JsonFixture::metadata_dep_name_collision();
    let mut metadata: serde_json::Value =
        serde_json::from_str(fixture.json()).expect("fixture is valid JSON");

    let main_node = metadata["resolve"]["nodes"]
        .as_array_mut()
        .expect("resolve.nodes is an array")
        .iter_mut()
        .find(|node| node["id"] == json::METADATA_DEP_NAME_COLLISION_MAIN)
        .expect("main is in the resolve");
    main_node["deps"]
        .as_array_mut()
        .expect("deps is an array")
        .reverse();

    let reversed = serde_json::to_string(&metadata).expect("metadata serializes");
    let reversed_graph = PackageGraph::from_json(&reversed).expect("reversed metadata builds");

    let original_order = link_order(fixture.graph());
    let reversed_order = link_order(&reversed_graph);
    assert_ne!(
        original_order, reversed_order,
        "reversing the resolve changes direct_links order"
    );
    assert_eq!(
        sorted_link_names(&reversed_graph),
        sorted_link_names(fixture.graph()),
        "reversing the resolve keeps the same links"
    );

    for case in CASES {
        case.check(
            &reversed_graph,
            json::METADATA_DEP_NAME_COLLISION_MAIN,
            "with reversed link order: ",
        );
    }
}

/// Returns a `(dep name, package name)` per direct link, in `direct_links`
/// order.
fn link_order(graph: &PackageGraph) -> Vec<(&str, &str)> {
    graph
        .metadata(&package_id(json::METADATA_DEP_NAME_COLLISION_MAIN))
        .expect("valid package ID")
        .direct_links()
        .map(|link| (link.dep_name(), link.to().name()))
        .collect()
}

fn sorted_link_names(graph: &PackageGraph) -> Vec<(&str, &str)> {
    let mut links = link_order(graph);
    links.sort();
    links
}
