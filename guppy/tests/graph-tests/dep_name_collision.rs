// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Tests for the case where one dependency name maps to several different
//! packages.
//!
//! This happens when a package declares one dependency name more than once,
//! for different targets or dependency kinds, and the declarations resolve to
//! different packages: either through `package = "..."` renames, or as
//! different versions of one package. Cargo then resolves features against
//! each of those packages:
//!
//! * `dep:name` is activated wherever any declaration of `name` applies.
//! * `name/feature` turns on `feature` in each package, under that package's
//!   own platform conditions.
//!
//! The `dep-name-collision` fixture's `main` package declares ten such names,
//! each exercising a different shape:
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
//! named = ["dep:named", "named/std"]
//! named-slash = ["named/std"]
//! devkinds-dep-colon = ["dep:devkinds"]
//! devkinds-slash = ["devkinds/std"]
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
//!
//! # A feature named after the dependency, as with semver 1.0.28's `serde`.
//! # One link per platform.
//! [target.'cfg(target_os = "linux")'.dependencies.named]
//! version = "1"
//! package = "fnv"
//! optional = true
//! default-features = false
//!
//! [target.'cfg(windows)'.dependencies.named]
//! version = "0.3"
//! package = "futures-sink"
//! optional = true
//! default-features = false
//!
//! # One link an optional normal dependency, the other a required
//! # dev-dependency.
//! [dependencies.devkinds]
//! version = "0.3"
//! package = "futures-core"
//! optional = true
//! default-features = false
//!
//! [dev-dependencies.devkinds]
//! version = "2"
//! package = "fastrand"
//! default-features = false
//! ```
//!
//! The expected results were obtained from Cargo 1.98.1 with
//! `cargo build --unit-graph -Z unstable-options`, under resolver version 2.

use crate::feature_helpers::{
    CargoResolutionCase, MACOS, SeenLink, VisitStatus, WINDOWS, conditional_links_from,
    graph_with_patched_json, specs, weak_edge_visits,
};
use fixtures::{
    json::{self, JsonFixture},
    package_id,
};
use guppy::{
    PackageId,
    errors::{FeatureBuildStage, FeatureGraphWarning},
    graph::{
        PackageGraph,
        feature::{FeatureId, LinkDeclarations},
    },
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
    // mixed = { package = "once_cell" }
    //
    // [target.'cfg(any())'.dependencies]
    // plain = { package = "byteorder" }
    //
    // [target.'cfg(unix)'.dependencies]
    // mixed = { package = "percent-encoding", optional = true }
    //
    // Neither of `plain`'s declarations is optional, so there is no
    // `dep:plain` feature and either is always built, even with no features
    // enabled.
    //
    // `mixed` resolves to two packages, one required and one optional. With no
    // features enabled, only once_cell is built.
    CargoResolutionCase::new(&[]).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,             Some("default")),
        (json::METADATA_DEP_NAME_COLLISION_EITHER,           Some("")),
        (json::METADATA_DEP_NAME_COLLISION_BYTEORDER,        None),
        (json::METADATA_DEP_NAME_COLLISION_ONCE_CELL,        Some("")),
        (json::METADATA_DEP_NAME_COLLISION_PERCENT_ENCODING, None),
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

    // [features]
    // named = ["dep:named", "named/std"]
    // named-slash = ["named/std"]
    //
    // `named` resolves to two packages:
    //
    // * fnv, declared under `cfg(target_os = "linux")`.
    // * futures-sink, declared under `cfg(windows)`.
    //
    // A non-weak `named/std` also activates the feature with the same name as
    // the dependency, `named`.
    CargoResolutionCase::new(&["named-slash"]).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,         Some("default named named-slash dep:named")),
        (json::METADATA_DEP_NAME_COLLISION_FNV,          Some("std")),
        (json::METADATA_DEP_NAME_COLLISION_FUTURES_SINK, None),
    ]),
    // Neither declaration applies on macOS, so neither `dep:named` nor the
    // feature `named` is activated.
    CargoResolutionCase::new(&["named-slash"]).target_platform(MACOS).target_expected(&[
        (json::METADATA_DEP_NAME_COLLISION_MAIN,         Some("default named-slash")),
        (json::METADATA_DEP_NAME_COLLISION_FNV,          None),
        (json::METADATA_DEP_NAME_COLLISION_FUTURES_SINK, None),
    ]),
];

/// Test the conditional links out of features that refer to a dependency name
/// with several packages: which package links each one is derived from, and
/// its platform statuses.
///
/// Links that share a dependency name are unioned in `direct_links` order, so
/// this is checked with both the original and the reversed link order.
#[test]
fn conditional_links_out_of_features() {
    let fixture = JsonFixture::metadata_dep_name_collision();
    let reversed_graph = reversed_link_order_graph();

    let main = package_id(json::METADATA_DEP_NAME_COLLISION_MAIN);
    let bytes = package_id(json::METADATA_DEP_NAME_COLLISION_BYTES);
    let bitflags = package_id(json::METADATA_DEP_NAME_COLLISION_BITFLAGS);
    let once_cell = package_id(json::METADATA_DEP_NAME_COLLISION_ONCE_CELL);
    let percent_encoding = package_id(json::METADATA_DEP_NAME_COLLISION_PERCENT_ENCODING);
    let fnv = package_id(json::METADATA_DEP_NAME_COLLISION_FNV);
    let futures_sink = package_id(json::METADATA_DEP_NAME_COLLISION_FUTURES_SINK);
    let futures_core = package_id(json::METADATA_DEP_NAME_COLLISION_FUTURES_CORE);
    let fastrand = package_id(json::METADATA_DEP_NAME_COLLISION_FASTRAND);

    let expected: Vec<(&str, Vec<ExpectedLink<'_>>)> = vec![
        (
            "dep-colon",
            vec![(
                FeatureId::optional_dependency(&main, "renamed"),
                vec!["bitflags", "bytes"],
                normal_only(LinkDeclarations::Unsplit, VisitStatus::Always),
            )],
        ),
        (
            "slash",
            vec![
                (
                    FeatureId::optional_dependency(&main, "renamed"),
                    vec!["bitflags", "bytes"],
                    normal_only(LinkDeclarations::Optional, VisitStatus::Always),
                ),
                (
                    FeatureId::named(&bitflags, "std"),
                    vec!["bitflags"],
                    normal_only(LinkDeclarations::Unsplit, specs(&["any()"])),
                ),
                (
                    FeatureId::named(&bytes, "std"),
                    vec!["bytes"],
                    normal_only(LinkDeclarations::Unsplit, VisitStatus::Always),
                ),
            ],
        ),
        (
            "mixed-slash",
            vec![
                // once_cell has no optional declaration, so the `Optional` link
                // to `dep:mixed` isn't derived from it.
                (
                    FeatureId::optional_dependency(&main, "mixed"),
                    vec!["percent-encoding"],
                    normal_only(LinkDeclarations::Optional, specs(&["unix"])),
                ),
                (
                    FeatureId::named(&once_cell, "std"),
                    vec!["once_cell"],
                    normal_only(LinkDeclarations::Unsplit, VisitStatus::Always),
                ),
                (
                    FeatureId::named(&percent_encoding, "std"),
                    vec!["percent-encoding"],
                    normal_only(LinkDeclarations::Unsplit, specs(&["unix"])),
                ),
            ],
        ),
        (
            "three-dep-colon",
            vec![(
                FeatureId::optional_dependency(&main, "three"),
                vec!["adler2", "anyhow", "foldhash"],
                normal_only(
                    LinkDeclarations::Unsplit,
                    specs(&["target_os = \"linux\"", "target_os = \"macos\"", "windows"]),
                ),
            )],
        ),
        (
            "kinds-dep-colon",
            vec![(
                FeatureId::optional_dependency(&main, "kinds"),
                vec!["hex", "log"],
                SeenLink {
                    declarations: LinkDeclarations::Unsplit,
                    normal: VisitStatus::Always,
                    build: VisitStatus::Always,
                    dev: VisitStatus::Never,
                },
            )],
        ),
        (
            "named-slash",
            vec![
                (
                    FeatureId::optional_dependency(&main, "named"),
                    vec!["fnv", "futures-sink"],
                    normal_only(
                        LinkDeclarations::Optional,
                        specs(&["target_os = \"linux\"", "windows"]),
                    ),
                ),
                (
                    FeatureId::named(&main, "named"),
                    vec!["fnv", "futures-sink"],
                    normal_only(
                        LinkDeclarations::Optional,
                        specs(&["target_os = \"linux\"", "windows"]),
                    ),
                ),
                (
                    FeatureId::named(&fnv, "std"),
                    vec!["fnv"],
                    normal_only(LinkDeclarations::Unsplit, specs(&["target_os = \"linux\""])),
                ),
                (
                    FeatureId::named(&futures_sink, "std"),
                    vec!["futures-sink"],
                    normal_only(LinkDeclarations::Unsplit, specs(&["windows"])),
                ),
            ],
        ),
        (
            "devkinds-dep-colon",
            vec![(
                FeatureId::optional_dependency(&main, "devkinds"),
                vec!["fastrand", "futures-core"],
                SeenLink {
                    declarations: LinkDeclarations::Unsplit,
                    normal: VisitStatus::Always,
                    build: VisitStatus::Never,
                    dev: VisitStatus::Always,
                },
            )],
        ),
        (
            "devkinds-slash",
            vec![
                (
                    FeatureId::optional_dependency(&main, "devkinds"),
                    vec!["futures-core"],
                    normal_only(LinkDeclarations::Optional, VisitStatus::Always),
                ),
                (
                    FeatureId::named(&fastrand, "std"),
                    vec!["fastrand"],
                    SeenLink {
                        declarations: LinkDeclarations::Unsplit,
                        normal: VisitStatus::Never,
                        build: VisitStatus::Never,
                        dev: VisitStatus::Always,
                    },
                ),
                (
                    FeatureId::named(&futures_core, "std"),
                    vec!["futures-core"],
                    normal_only(LinkDeclarations::Unsplit, VisitStatus::Always),
                ),
            ],
        ),
    ];

    for (graph, link_order) in [(fixture.graph(), "original"), (&reversed_graph, "reversed")] {
        for (from_feature, expected_links) in &expected {
            let mut expected_links = expected_links.clone();
            sort_links(&mut expected_links);
            assert_eq!(
                links_out_of(graph, &main, from_feature),
                expected_links,
                "with {link_order} link order, links out of {from_feature} match"
            );
        }
    }
}

/// Test that a weak `name?/feature` is weak separately for each package the
/// name resolves to.
///
/// Each link with an optional declaration is held back until `dep:name` is
/// activated. A link without one, like once_cell's, isn't weak at all.
#[test]
fn weak_edges_are_per_link() {
    let graph = JsonFixture::metadata_dep_name_collision().graph();
    let libc = package_id(json::METADATA_DEP_NAME_COLLISION_LIBC);
    let memchr = package_id(json::METADATA_DEP_NAME_COLLISION_MEMCHR);
    let once_cell = package_id(json::METADATA_DEP_NAME_COLLISION_ONCE_CELL);
    let percent_encoding = package_id(json::METADATA_DEP_NAME_COLLISION_PERCENT_ENCODING);

    let visits = |features, from_feature, to_package| {
        weak_edge_visits(
            graph,
            json::METADATA_DEP_NAME_COLLISION_MAIN,
            features,
            from_feature,
            to_package,
            "std",
        )
    };

    for to_package in [&libc, &memchr] {
        assert_eq!(
            visits("both-weak-slash", "both-weak-slash", to_package),
            Vec::<SeenLink>::new(),
            "without dep:both, the weak edge to {to_package}/std isn't visited"
        );
    }
    assert_eq!(
        visits("both-weak-slash both-dep-colon", "both-weak-slash", &libc),
        [normal_only(LinkDeclarations::Optional, VisitStatus::Always)],
        "dep:both releases the weak edge to libc/std"
    );
    assert_eq!(
        visits("both-weak-slash both-dep-colon", "both-weak-slash", &memchr),
        [normal_only(LinkDeclarations::Optional, specs(&["unix"]))],
        "dep:both releases the weak edge to memchr/std"
    );

    assert_eq!(
        visits("mixed-weak-slash", "mixed-weak-slash", &once_cell),
        [normal_only(LinkDeclarations::Unsplit, VisitStatus::Always)],
        "once_cell has no optional declaration, so its edge isn't weak"
    );
    assert_eq!(
        visits("mixed-weak-slash", "mixed-weak-slash", &percent_encoding),
        Vec::<SeenLink>::new(),
        "without dep:mixed, the weak edge to percent-encoding/std isn't visited"
    );
}

/// Test that `name/feature` warns about each package under the name that is
/// missing `feature`, even if another package under the name has it.
#[test]
fn missing_feature_warns_per_link() {
    let graph = graph_with_patched_json(JsonFixture::metadata_dep_name_collision(), |metadata| {
        let bitflags = metadata["packages"]
            .as_array_mut()
            .expect("packages is an array")
            .iter_mut()
            .find(|package| package["id"] == json::METADATA_DEP_NAME_COLLISION_BITFLAGS)
            .expect("bitflags is in the fixture");
        bitflags["features"]
            .as_object_mut()
            .expect("features is a map")
            .remove("std")
            .expect("bitflags has a std feature");
    });
    let main = package_id(json::METADATA_DEP_NAME_COLLISION_MAIN);
    let bitflags = package_id(json::METADATA_DEP_NAME_COLLISION_BITFLAGS);

    let mut actual = graph.feature_graph().build_warnings().to_vec();
    actual.sort();
    let mut expected: Vec<_> = ["slash", "weak-slash"]
        .into_iter()
        .map(|from_feature| FeatureGraphWarning::MissingFeature {
            stage: FeatureBuildStage::AddNamedFeatureEdges {
                package_id: main.clone(),
                from_feature: from_feature.to_owned(),
            },
            package_id: bitflags.clone(),
            feature_name: "std".to_owned(),
        })
        .collect();
    expected.sort();
    assert_eq!(
        actual, expected,
        "renamed/std and renamed?/std warn for bitflags, but not for bytes"
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
    let reversed_graph = reversed_link_order_graph();

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

/// A conditional link: the feature it leads to, the sorted names of the
/// packages its package links end at, and its statuses.
type ExpectedLink<'a> = (FeatureId<'a>, Vec<&'a str>, SeenLink);

/// Returns the fixture with `main`'s resolve dependencies reversed, which
/// reverses the order of the links under each dependency name.
fn reversed_link_order_graph() -> PackageGraph {
    graph_with_patched_json(JsonFixture::metadata_dep_name_collision(), |metadata| {
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
    })
}

/// Returns the conditional links out of `main/<from_feature>`, sorted.
fn links_out_of<'g>(
    graph: &'g PackageGraph,
    main: &'g PackageId,
    from_feature: &'g str,
) -> Vec<ExpectedLink<'g>> {
    let mut links: Vec<_> = conditional_links_from(graph, FeatureId::named(main, from_feature))
        .into_iter()
        .map(|link| {
            let mut to_names: Vec<_> = link
                .package_links()
                .map(|package_link| package_link.to().name())
                .collect();
            to_names.sort_unstable();
            (link.to().feature_id(), to_names, SeenLink::from_link(&link))
        })
        .collect();
    sort_links(&mut links);
    links
}

fn sort_links(links: &mut [ExpectedLink<'_>]) {
    links.sort_by(|(a_to, a_names, _), (b_to, b_names, _)| (a_to, a_names).cmp(&(b_to, b_names)));
}

fn normal_only(declarations: LinkDeclarations, normal: VisitStatus) -> SeenLink {
    SeenLink {
        declarations,
        normal,
        build: VisitStatus::Never,
        dev: VisitStatus::Never,
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
