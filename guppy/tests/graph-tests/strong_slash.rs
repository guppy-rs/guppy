// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Tests for dependency features without a `?` (`dep/feature`), on a
//! dependency with required declarations.
//!
//! The `builddep` fixture's `main` has a build script. The parts of its
//! manifest exercised here are:
//!
//! ```toml
//! [features]
//! platdep-slash = ["platdep/std"]
//! builddep = []
//! builddep-slash = ["builddep/std"]
//!
//! [build-dependencies]
//! builddep = { path = "../builddep" }
//!
//! [target.'cfg(unix)'.dependencies]
//! platdep = { path = "../platdep" }
//!
//! [target.'cfg(windows)'.dependencies]
//! platdep = { path = "../platdep", optional = true }
//! ```
//!
//! `platdep` and `builddep` each have features `alloc = []` and
//! `std = ["alloc"]`. The optional `platdep` dependency is not referred to with
//! `dep:`, so it has an implicit feature of the same name.
//!
//! `foo/std` enables `std` on every declaration of `foo` that applies. It also
//! activates `dep:foo` and a feature named `foo`, if there is one, but only
//! through an optional declaration of `foo` that applies.
//!
//! The expected results were obtained from Cargo 1.98.1 with
//! `cargo build --unit-graph -Z unstable-options`, under both resolver
//! versions 1 and 2.

use crate::feature_helpers::{CargoResolutionCase, WINDOWS};
use fixtures::json::{self, JsonFixture};
use guppy::graph::cargo::CargoResolverVersion;

#[rustfmt::skip]
static CASES: &[CargoResolutionCase] = &[
    // [features]
    // platdep-slash = ["platdep/std"]
    //
    // platdep is required on Unix, so `platdep/std` enables platdep's `std`.
    // The optional declaration is `cfg(windows)`-only, so `main` doesn't get
    // `platdep` or `dep:platdep`.
    CargoResolutionCase::new(&["platdep-slash"])
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("platdep-slash")),
            (json::METADATA_BUILDDEP_PLATDEP,       Some("alloc std")),
        ]),
    // The v1 resolver doesn't filter features by platform, so the optional
    // declaration applies, and `main` gets `platdep` and `dep:platdep`.
    CargoResolutionCase::new(&["platdep-slash"])
        .resolver(CargoResolverVersion::V1)
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("platdep platdep-slash dep:platdep")),
            (json::METADATA_BUILDDEP_PLATDEP,       Some("alloc std")),
        ]),
    // On Windows, platdep is optional, and `platdep/std` activates the
    // optional declaration.
    CargoResolutionCase::new(&["platdep-slash"])
        .target_platform(WINDOWS)
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("platdep platdep-slash dep:platdep")),
            (json::METADATA_BUILDDEP_PLATDEP,       Some("alloc std")),
        ]),
    // The same with the v1 resolver.
    CargoResolutionCase::new(&["platdep-slash"])
        .resolver(CargoResolverVersion::V1)
        .target_platform(WINDOWS)
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("platdep platdep-slash dep:platdep")),
            (json::METADATA_BUILDDEP_PLATDEP,       Some("alloc std")),
        ]),

    // [features]
    // builddep = []
    // builddep-slash = ["builddep/std"]
    //
    // builddep is a required build dependency, so it gets `std` on the host.
    // It has no optional declarations, so the `builddep` feature, which only
    // shares its name with the dependency, is not enabled.
    CargoResolutionCase::new(&["builddep-slash"])
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("builddep-slash")),
            (json::METADATA_BUILDDEP_BUILDDEP,      None),
        ])
        .host_expected(&[
            (json::METADATA_BUILDDEP_BUILDDEP,      Some("alloc std")),
        ]),
    // The same with the v1 resolver.
    CargoResolutionCase::new(&["builddep-slash"])
        .resolver(CargoResolverVersion::V1)
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("builddep-slash")),
            (json::METADATA_BUILDDEP_BUILDDEP,      None),
        ])
        .host_expected(&[
            (json::METADATA_BUILDDEP_BUILDDEP,      Some("alloc std")),
        ]),
];

#[test]
fn resolution_matches_cargo() {
    let graph = JsonFixture::metadata_builddep().graph();
    for case in CASES {
        case.check(graph, json::METADATA_BUILDDEP_MAIN, "");
    }
}
