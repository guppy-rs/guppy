// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Tests for the case where one dependency name maps to several different
//! packages.
//!
//! See `add_named_feature_edges` in `guppy/src/graph/feature/build.rs` for how
//! this arises, and why every `dep:name` and `name/feature` entry has to be
//! resolved against all of the links.
//!
//! The `dep-name-collision` fixture's `main` package declares eight such names,
//! one per interesting combination of platforms, optionality and dependency
//! kinds:
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
//! base64-dep-colon = ["dep:base64"]
//! base64-slash = ["base64/std"]
//! mixed-dep-colon = ["dep:mixed"]
//! mixed-slash = ["mixed/std"]
//! mixed-weak-slash = ["mixed?/std"]
//! three-dep-colon = ["dep:three"]
//! three-slash = ["three/std"]
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
//! # Both links live at once on Unix.
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
//!
//! # Two versions of one package under one name, with no rename.
//! [dependencies.base64]
//! version = "0.21"
//! optional = true
//! default-features = false
//!
//! [target.'cfg(windows)'.dependencies.base64]
//! version = "0.22"
//! optional = true
//! default-features = false
//!
//! # One link required everywhere, one link optional on Unix.
//! [dependencies.mixed]
//! version = "1"
//! package = "once_cell"
//! default-features = false
//!
//! [target.'cfg(unix)'.dependencies.mixed]
//! version = "2"
//! package = "percent-encoding"
//! optional = true
//! default-features = false
//!
//! # Three links, one per platform: all three conditions have to be unioned.
//! [target.'cfg(target_os = "linux")'.dependencies.three]
//! version = "1"
//! package = "anyhow"
//! optional = true
//! default-features = false
//!
//! [target.'cfg(windows)'.dependencies.three]
//! version = "0.1"
//! package = "foldhash"
//! optional = true
//! default-features = false
//!
//! [target.'cfg(target_os = "macos")'.dependencies.three]
//! version = "2"
//! package = "adler2"
//! optional = true
//! default-features = false
//! ```
//!
//! The expected results were obtained from Cargo 1.98.1 with
//! `cargo build --unit-graph -Z unstable-options`, under resolver version 2.

use crate::feature_helpers::{
    CargoResolutionCase, MACOS, SeenLink, VisitStatus, WINDOWS, conditional_links_from,
    graph_with_patched_json, specs,
};
use fixtures::{
    json::{self, JsonFixture},
    package_id,
};
use guppy::graph::{
    PackageGraph,
    feature::{FeatureId, LinkDeclarations},
};
use std::collections::BTreeMap;

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

    // [features]
    // split-dep-colon = ["dep:split"]
    //
    // `split` resolves to arrayvec on Linux and tinyvec on Windows. Neither
    // declaration applies on macOS, so `dep:split` isn't activated there and
    // neither package is built.
    CargoResolutionCase::new(&["split-dep-colon"]).target_platform(MACOS).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,     Some("default split-dep-colon")),
        (json::METADATA_DEP_NAME_COLLISION_ARRAYVEC, None),
        (json::METADATA_DEP_NAME_COLLISION_TINYVEC,  None),
    ]),
    // [features]
    // split-slash = ["split/std"]
    //
    // The same for `split/std` on macOS.
    CargoResolutionCase::new(&["split-slash"]).target_platform(MACOS).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,     Some("default split-slash")),
        (json::METADATA_DEP_NAME_COLLISION_ARRAYVEC, None),
        (json::METADATA_DEP_NAME_COLLISION_TINYVEC,  None),
    ]),

    // [features]
    // base64-dep-colon = ["dep:base64"]
    //
    // `base64` resolves to two versions of one package, with no rename:
    //
    // * base64 0.21 as a plain (non-target) dependency.
    // * base64 0.22, declared under `cfg(windows)`.
    //
    // On Linux, only 0.21 is built.
    CargoResolutionCase::new(&["base64-dep-colon"]).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,        Some("base64-dep-colon default dep:base64")),
        (json::METADATA_DEP_NAME_COLLISION_BASE64_0_21, Some("")),
        (json::METADATA_DEP_NAME_COLLISION_BASE64_0_22, None),
    ]),
    // On Windows, both versions are built.
    CargoResolutionCase::new(&["base64-dep-colon"]).target_platform(WINDOWS).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,        Some("base64-dep-colon default dep:base64")),
        (json::METADATA_DEP_NAME_COLLISION_BASE64_0_21, Some("")),
        (json::METADATA_DEP_NAME_COLLISION_BASE64_0_22, Some("")),
    ]),
    // [features]
    // base64-slash = ["base64/std"]
    //
    // `base64` resolves to base64 0.21 and 0.22 as above. `std` implies
    // `alloc` in both versions.
    CargoResolutionCase::new(&["base64-slash"]).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,        Some("base64-slash default dep:base64")),
        (json::METADATA_DEP_NAME_COLLISION_BASE64_0_21, Some("alloc std")),
        (json::METADATA_DEP_NAME_COLLISION_BASE64_0_22, None),
    ]),
    // The same on Windows, where both versions get `std`.
    CargoResolutionCase::new(&["base64-slash"]).target_platform(WINDOWS).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,        Some("base64-slash default dep:base64")),
        (json::METADATA_DEP_NAME_COLLISION_BASE64_0_21, Some("alloc std")),
        (json::METADATA_DEP_NAME_COLLISION_BASE64_0_22, Some("alloc std")),
    ]),

    // [dependencies]
    // mixed = { package = "once_cell" }
    //
    // [target.'cfg(unix)'.dependencies]
    // mixed = { package = "percent-encoding", optional = true }
    //
    // `mixed` resolves to two packages, one required and one optional. With no
    // features enabled, only once_cell is built.
    CargoResolutionCase::new(&[]).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,             Some("default")),
        (json::METADATA_DEP_NAME_COLLISION_ONCE_CELL,        Some("")),
        (json::METADATA_DEP_NAME_COLLISION_PERCENT_ENCODING, None),
    ]),
    // [features]
    // mixed-weak-slash = ["mixed?/std"]
    //
    // once_cell is required, so the weak `mixed?/std` applies to it as if it
    // were `mixed/std`. once_cell's `std` implies `alloc`, which implies
    // `race`. percent-encoding is optional, and nothing activates `dep:mixed`,
    // so it isn't built.
    CargoResolutionCase::new(&["mixed-weak-slash"]).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,             Some("default mixed-weak-slash")),
        (json::METADATA_DEP_NAME_COLLISION_ONCE_CELL,        Some("alloc race std")),
        (json::METADATA_DEP_NAME_COLLISION_PERCENT_ENCODING, None),
    ]),
    // [features]
    // mixed-weak-slash = ["mixed?/std"]
    // mixed-dep-colon = ["dep:mixed"]
    //
    // Once `dep:mixed` activates the dependency, the buffered weak edge flushes
    // and percent-encoding gets `std`, which implies `alloc`.
    CargoResolutionCase::new(&["mixed-weak-slash", "mixed-dep-colon"]).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,             Some("default mixed-dep-colon mixed-weak-slash dep:mixed")),
        (json::METADATA_DEP_NAME_COLLISION_ONCE_CELL,        Some("alloc race std")),
        (json::METADATA_DEP_NAME_COLLISION_PERCENT_ENCODING, Some("alloc std")),
    ]),
    // The same on Windows, where percent-encoding's `cfg(unix)` link is off.
    CargoResolutionCase::new(&["mixed-weak-slash", "mixed-dep-colon"]).target_platform(WINDOWS).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,             Some("default mixed-dep-colon mixed-weak-slash dep:mixed")),
        (json::METADATA_DEP_NAME_COLLISION_ONCE_CELL,        Some("alloc race std")),
        (json::METADATA_DEP_NAME_COLLISION_PERCENT_ENCODING, None),
    ]),
    // [features]
    // mixed-slash = ["mixed/std"]
    //
    // A non-weak `mixed/std` activates `dep:mixed` through percent-encoding's
    // optional declaration, and turns on `std` in both packages.
    CargoResolutionCase::new(&["mixed-slash"]).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,             Some("default mixed-slash dep:mixed")),
        (json::METADATA_DEP_NAME_COLLISION_ONCE_CELL,        Some("alloc race std")),
        (json::METADATA_DEP_NAME_COLLISION_PERCENT_ENCODING, Some("alloc std")),
    ]),
    // On Windows, percent-encoding's optional declaration doesn't apply, so
    // `dep:mixed` isn't activated and only once_cell gets `std`.
    CargoResolutionCase::new(&["mixed-slash"]).target_platform(WINDOWS).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,             Some("default mixed-slash")),
        (json::METADATA_DEP_NAME_COLLISION_ONCE_CELL,        Some("alloc race std")),
        (json::METADATA_DEP_NAME_COLLISION_PERCENT_ENCODING, None),
    ]),

    // [features]
    // three-dep-colon = ["dep:three"]
    //
    // `three` resolves to three packages, one per platform:
    //
    // * anyhow, declared under `cfg(target_os = "linux")`.
    // * foldhash, declared under `cfg(windows)`.
    // * adler2, declared under `cfg(target_os = "macos")`.
    //
    // All three conditions are unioned, so `dep:three` activates on each
    // platform, pulling in only the package that is buildable there.
    CargoResolutionCase::new(&["three-dep-colon"]).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,     Some("default three-dep-colon dep:three")),
        (json::METADATA_DEP_NAME_COLLISION_ANYHOW,   Some("")),
        (json::METADATA_DEP_NAME_COLLISION_FOLDHASH, None),
        (json::METADATA_DEP_NAME_COLLISION_ADLER2,   None),
    ]),
    // The same on Windows.
    CargoResolutionCase::new(&["three-dep-colon"]).target_platform(WINDOWS).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,     Some("default three-dep-colon dep:three")),
        (json::METADATA_DEP_NAME_COLLISION_ANYHOW,   None),
        (json::METADATA_DEP_NAME_COLLISION_FOLDHASH, Some("")),
        (json::METADATA_DEP_NAME_COLLISION_ADLER2,   None),
    ]),
    // The same on macOS.
    CargoResolutionCase::new(&["three-dep-colon"]).target_platform(MACOS).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,     Some("default three-dep-colon dep:three")),
        (json::METADATA_DEP_NAME_COLLISION_ANYHOW,   None),
        (json::METADATA_DEP_NAME_COLLISION_FOLDHASH, None),
        (json::METADATA_DEP_NAME_COLLISION_ADLER2,   Some("")),
    ]),
    // [features]
    // three-slash = ["three/std"]
    //
    // `three` resolves to anyhow, foldhash and adler2 as above.
    CargoResolutionCase::new(&["three-slash"]).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,     Some("default three-slash dep:three")),
        (json::METADATA_DEP_NAME_COLLISION_ANYHOW,   Some("std")),
        (json::METADATA_DEP_NAME_COLLISION_FOLDHASH, None),
        (json::METADATA_DEP_NAME_COLLISION_ADLER2,   None),
    ]),
    // The same on Windows.
    CargoResolutionCase::new(&["three-slash"]).target_platform(WINDOWS).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,     Some("default three-slash dep:three")),
        (json::METADATA_DEP_NAME_COLLISION_ANYHOW,   None),
        (json::METADATA_DEP_NAME_COLLISION_FOLDHASH, Some("std")),
        (json::METADATA_DEP_NAME_COLLISION_ADLER2,   None),
    ]),
    // The same on macOS.
    CargoResolutionCase::new(&["three-slash"]).target_platform(MACOS).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,     Some("default three-slash dep:three")),
        (json::METADATA_DEP_NAME_COLLISION_ANYHOW,   None),
        (json::METADATA_DEP_NAME_COLLISION_FOLDHASH, None),
        (json::METADATA_DEP_NAME_COLLISION_ADLER2,   Some("std")),
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
        let mut links: Vec<_> = conditional_links_from(graph, from)
            .into_iter()
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

    // TODO-RAINCLAUDE: once_cell has no optional declaration, so the Optional
    // link to dep:mixed must not report it.
    let once_cell = package_id(json::METADATA_DEP_NAME_COLLISION_ONCE_CELL);
    let percent_encoding = package_id(json::METADATA_DEP_NAME_COLLISION_PERCENT_ENCODING);
    assert_eq!(
        links_out_of("mixed-slash"),
        [
            (
                FeatureId::optional_dependency(&main, "mixed"),
                vec!["percent-encoding"],
            ),
            (FeatureId::named(&once_cell, "std"), vec!["once_cell"]),
            (
                FeatureId::named(&percent_encoding, "std"),
                vec!["percent-encoding"],
            ),
        ],
        "dep:mixed is derived only from the link with an optional declaration"
    );
}

/// Test that the link to `dep:name` unions the platform statuses of every
/// package the name resolves to.
#[test]
fn dep_colon_link_statuses_are_unioned() {
    let graph = JsonFixture::metadata_dep_name_collision().graph();
    let main = package_id(json::METADATA_DEP_NAME_COLLISION_MAIN);

    let dep_link = |feature, dep_name| {
        let to = FeatureId::optional_dependency(&main, dep_name);
        let links: Vec<_> = conditional_links_from(graph, FeatureId::named(&main, feature))
            .iter()
            .filter(|link| link.to().feature_id() == to)
            .map(SeenLink::from_link)
            .collect();
        links
    };

    // [target.'cfg(target_os = "linux")'.dependencies]
    // split = { package = "arrayvec", optional = true }
    //
    // [target.'cfg(windows)'.dependencies]
    // split = { package = "tinyvec", optional = true }
    //
    // [features]
    // split-dep-colon = ["dep:split"]
    //
    // `dep:split` is enabled on Linux and on Windows.
    assert_eq!(
        dep_link("split-dep-colon", "split"),
        [SeenLink {
            declarations: LinkDeclarations::Unsplit,
            normal: specs(&["target_os = \"linux\"", "windows"]),
            build: VisitStatus::Never,
            dev: VisitStatus::Never,
        }],
        "dep:split is enabled wherever arrayvec or tinyvec is"
    );

    // [target.'cfg(target_os = "linux")'.dependencies]
    // three = { package = "anyhow", optional = true }
    //
    // [target.'cfg(windows)'.dependencies]
    // three = { package = "foldhash", optional = true }
    //
    // [target.'cfg(target_os = "macos")'.dependencies]
    // three = { package = "adler2", optional = true }
    //
    // [features]
    // three-dep-colon = ["dep:three"]
    //
    // `dep:three` is enabled on all three platforms.
    assert_eq!(
        dep_link("three-dep-colon", "three"),
        [SeenLink {
            declarations: LinkDeclarations::Unsplit,
            normal: specs(&["target_os = \"linux\"", "target_os = \"macos\"", "windows",]),
            build: VisitStatus::Never,
            dev: VisitStatus::Never,
        }],
        "dep:three is enabled wherever anyhow, foldhash or adler2 is"
    );

    // [dependencies]
    // mixed = { package = "once_cell" }
    //
    // [target.'cfg(unix)'.dependencies]
    // mixed = { package = "percent-encoding", optional = true }
    //
    // [features]
    // mixed-slash = ["mixed/std"]
    //
    // `mixed/std` activates `dep:mixed` only through the optional declaration,
    // so the link is enabled on Unix alone.
    assert_eq!(
        dep_link("mixed-slash", "mixed"),
        [SeenLink {
            declarations: LinkDeclarations::Optional,
            normal: specs(&["unix"]),
            build: VisitStatus::Never,
            dev: VisitStatus::Never,
        }],
        "dep:mixed is enabled wherever percent-encoding's optional declaration is"
    );

    // [dependencies]
    // kinds = { package = "log", optional = true }
    //
    // [build-dependencies]
    // kinds = { package = "hex", optional = true }
    //
    // [features]
    // kinds-dep-colon = ["dep:kinds"]
    //
    // log contributes the normal status and hex the build status.
    assert_eq!(
        dep_link("kinds-dep-colon", "kinds"),
        [SeenLink {
            declarations: LinkDeclarations::Unsplit,
            normal: VisitStatus::Always,
            build: VisitStatus::Always,
            dev: VisitStatus::Never,
        }],
        "dep:kinds is enabled as a normal dependency and a build dependency"
    );
}

#[test]
fn resolution_matches_cargo() {
    let graph = JsonFixture::metadata_dep_name_collision().graph();
    for case in CASES {
        case.check(graph, json::METADATA_DEP_NAME_COLLISION_MAIN, "");
    }
}

// Links that share a dependency name are merged in `direct_links` order, which
// follows the order of `resolve.nodes[].deps`. Results must not depend on that
// order.
#[test]
fn resolution_independent_of_link_order() {
    let fixture = JsonFixture::metadata_dep_name_collision();
    let reversed_graph = graph_with_patched_json(fixture, |metadata| {
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
    });

    let original = links_by_dep_name(fixture.graph());
    let reversed = links_by_dep_name(&reversed_graph);
    assert_eq!(
        original.keys().collect::<Vec<_>>(),
        reversed.keys().collect::<Vec<_>>(),
        "reversing the resolve keeps the same dependency names"
    );
    for (dep_name, links) in &original {
        let mut expected: Vec<_> = links.clone();
        expected.reverse();
        assert_eq!(
            reversed[dep_name], expected,
            "for {dep_name}, reversing the resolve reverses the order of its links"
        );
    }

    for case in CASES {
        case.check(
            &reversed_graph,
            json::METADATA_DEP_NAME_COLLISION_MAIN,
            "with reversed link order: ",
        );
    }
}

/// Returns the package IDs of `main`'s links for each dependency name, in
/// `direct_links` order.
fn links_by_dep_name(graph: &PackageGraph) -> BTreeMap<&str, Vec<&str>> {
    let mut links: BTreeMap<_, Vec<_>> = BTreeMap::new();
    for link in graph
        .metadata(&package_id(json::METADATA_DEP_NAME_COLLISION_MAIN))
        .expect("valid package ID")
        .direct_links()
    {
        links
            .entry(link.dep_name())
            .or_default()
            .push(link.to().id().repr());
    }
    links
}
