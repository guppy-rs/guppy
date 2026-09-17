// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Tests for optional build dependencies activated through
//! their own package's features.
//!
//! The `builddep` fixture's `main` has a build script and this manifest:
//!
//! ```toml
//! [features]
//! dep-colon = ["dep:optbuilddep"]
//! slash = ["optbuilddep/std"]
//! windows-dep-colon = ["dep:winbuilddep"]
//!
//! [build-dependencies]
//! builddep = { path = "../builddep" }
//! optbuilddep = { path = "../optbuilddep", optional = true }
//!
//! [target.'cfg(windows)'.build-dependencies]
//! winbuilddep = { path = "../winbuilddep", optional = true }
//! ```
//!
//! `optbuilddep` has features `alloc = []` and `std = ["alloc"]`.

use crate::feature_helpers::{CargoResolutionCase, WINDOWS};
use fixtures::json::{self, JsonFixture};
use guppy::graph::cargo::CargoResolverVersion;

#[rustfmt::skip]
static CASES: &[CargoResolutionCase] = &[
    // Nothing enabled, so no build dependency is built.
    CargoResolutionCase::new(&[])
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("")),
            (json::METADATA_BUILDDEP_OPTBUILDDEP,   None),
        ])
        .host_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          None),
            (json::METADATA_BUILDDEP_OPTBUILDDEP,   None),
        ]),

    // [features]
    // dep-colon = ["dep:optbuilddep"]
    //
    // The edge from `dep-colon` to `dep:optbuilddep` stays within `main` and
    // its only enabled kind is build. It has to be followed on the target so
    // that optbuilddep is built on the host; `main` itself is not built on the
    // host.
    CargoResolutionCase::new(&["dep-colon"])
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("dep-colon dep:optbuilddep")),
            (json::METADATA_BUILDDEP_OPTBUILDDEP,   None),
        ])
        .host_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          None),
            (json::METADATA_BUILDDEP_OPTBUILDDEP,   Some("")),
        ]),
    // The same with the v1 resolver.
    CargoResolutionCase::new(&["dep-colon"])
        .resolver(CargoResolverVersion::V1)
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("dep-colon dep:optbuilddep")),
            (json::METADATA_BUILDDEP_OPTBUILDDEP,   None),
        ])
        .host_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          None),
            (json::METADATA_BUILDDEP_OPTBUILDDEP,   Some("")),
        ]),

    // [features]
    // slash = ["optbuilddep/std"]
    //
    // optbuilddep's `std` implies `alloc`.
    CargoResolutionCase::new(&["slash"])
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("slash dep:optbuilddep")),
            (json::METADATA_BUILDDEP_OPTBUILDDEP,   None),
        ])
        .host_expected(&[
            (json::METADATA_BUILDDEP_OPTBUILDDEP,   Some("alloc std")),
        ]),

    // [features]
    // windows-dep-colon = ["dep:winbuilddep"]
    //
    // Build dependencies are evaluated against the host platform. On a Linux
    // host the `cfg(windows)` declaration does not apply, so `dep:winbuilddep`
    // activates nothing.
    CargoResolutionCase::new(&["windows-dep-colon"])
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("windows-dep-colon")),
            (json::METADATA_BUILDDEP_WINBUILDDEP,   None),
        ])
        .host_expected(&[
            (json::METADATA_BUILDDEP_WINBUILDDEP,   None),
        ]),
    // On a Windows host it does apply.
    CargoResolutionCase::new(&["windows-dep-colon"])
        .host_platform(WINDOWS)
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("windows-dep-colon dep:winbuilddep")),
            (json::METADATA_BUILDDEP_WINBUILDDEP,   None),
        ])
        .host_expected(&[
            (json::METADATA_BUILDDEP_WINBUILDDEP,   Some("")),
        ]),
];

#[test]
fn resolution_matches_cargo() {
    let graph = JsonFixture::metadata_builddep().graph();
    for case in CASES {
        case.check(graph, json::METADATA_BUILDDEP_MAIN, "");
    }
}
