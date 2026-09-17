// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Tests for weak dependency features (`dep?/feature`) on a dependency that is
//! required in one declaration and optional in another.
//!
//! The `builddep` fixture's `main` has a build script. The parts of its
//! manifest exercised here are:
//!
//! ```toml
//! [features]
//! platdep-weak = ["platdep?/std"]
//! normaldep-weak = ["normaldep?/std"]
//! reqbuilddep-weak = ["reqbuilddep?/std"]
//!
//! [dependencies]
//! normaldep = { path = "../normaldep" }
//! reqbuilddep = { path = "../reqbuilddep", optional = true }
//!
//! [build-dependencies]
//! normaldep = { path = "../normaldep", optional = true }
//! reqbuilddep = { path = "../reqbuilddep" }
//!
//! [target.'cfg(unix)'.dependencies]
//! platdep = { path = "../platdep" }
//!
//! [target.'cfg(windows)'.dependencies]
//! platdep = { path = "../platdep", optional = true }
//! ```
//!
//! `platdep`, `normaldep` and `reqbuilddep` each have features `alloc = []`
//! and `std = ["alloc"]`. None of these optional dependencies are referred to
//! with `dep:`, so each has an implicit feature of the same name.
//!
//! The expected results were obtained from Cargo 1.98.1 with
//! `cargo build --unit-graph -Z unstable-options`, under both resolver
//! versions 1 and 2.

use crate::feature_helpers::{CargoResolutionCase, WINDOWS};
use fixtures::json::{self, JsonFixture};
use guppy::graph::cargo::CargoResolverVersion;

#[rustfmt::skip]
static CASES: &[CargoResolutionCase] = &[
    // Nothing enabled. The required declarations are built, and the optional
    // ones are not.
    CargoResolutionCase::new(&[])
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("")),
            (json::METADATA_BUILDDEP_PLATDEP,       Some("")),
            (json::METADATA_BUILDDEP_NORMALDEP,     Some("")),
            (json::METADATA_BUILDDEP_REQBUILDDEP,   None),
        ])
        .host_expected(&[
            (json::METADATA_BUILDDEP_NORMALDEP,     None),
            (json::METADATA_BUILDDEP_REQBUILDDEP,   Some("")),
        ]),

    // [features]
    // platdep-weak = ["platdep?/std"]
    //
    // platdep is required on Unix, so `platdep?/std` enables platdep's `std`.
    // It does not activate the optional declaration, so `main` doesn't get
    // `dep:platdep`.
    CargoResolutionCase::new(&["platdep-weak"])
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("platdep-weak")),
            (json::METADATA_BUILDDEP_PLATDEP,       Some("alloc std")),
        ]),
    // The same with the v1 resolver.
    CargoResolutionCase::new(&["platdep-weak"])
        .resolver(CargoResolverVersion::V1)
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("platdep-weak")),
            (json::METADATA_BUILDDEP_PLATDEP,       Some("alloc std")),
        ]),
    // On Windows, platdep is optional and nothing activates it.
    CargoResolutionCase::new(&["platdep-weak"])
        .target_platform(WINDOWS)
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("platdep-weak")),
            (json::METADATA_BUILDDEP_PLATDEP,       None),
        ]),
    // [features]
    // platdep-weak = ["platdep?/std"]
    // platdep = ["dep:platdep"]
    //
    // On Windows, enabling `platdep` activates the optional declaration, and
    // `platdep?/std` then applies.
    CargoResolutionCase::new(&["platdep-weak", "platdep"])
        .target_platform(WINDOWS)
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("platdep platdep-weak dep:platdep")),
            (json::METADATA_BUILDDEP_PLATDEP,       Some("alloc std")),
        ]),

    // [features]
    // normaldep-weak = ["normaldep?/std"]
    //
    // normaldep is a required normal dependency, so it gets `std` on the
    // target. The optional build dependency is not activated, so normaldep is
    // not built on the host.
    CargoResolutionCase::new(&["normaldep-weak"])
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("normaldep-weak")),
            (json::METADATA_BUILDDEP_NORMALDEP,     Some("alloc std")),
        ])
        .host_expected(&[
            (json::METADATA_BUILDDEP_NORMALDEP,     None),
        ]),
    // The same with the v1 resolver.
    CargoResolutionCase::new(&["normaldep-weak"])
        .resolver(CargoResolverVersion::V1)
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("normaldep-weak")),
            (json::METADATA_BUILDDEP_NORMALDEP,     Some("alloc std")),
        ])
        .host_expected(&[
            (json::METADATA_BUILDDEP_NORMALDEP,     None),
        ]),
    // [features]
    // normaldep-weak = ["normaldep?/std"]
    // normaldep = ["dep:normaldep"]
    //
    // Enabling `normaldep` activates the optional build dependency, and
    // `normaldep?/std` then applies on the host as well.
    CargoResolutionCase::new(&["normaldep-weak", "normaldep"])
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("normaldep normaldep-weak dep:normaldep")),
            (json::METADATA_BUILDDEP_NORMALDEP,     Some("alloc std")),
        ])
        .host_expected(&[
            (json::METADATA_BUILDDEP_NORMALDEP,     Some("alloc std")),
        ]),

    // [features]
    // reqbuilddep-weak = ["reqbuilddep?/std"]
    //
    // The reverse of `normaldep-weak`: reqbuilddep is a required build
    // dependency, so it gets `std` on the host. The optional normal dependency
    // is not activated, so reqbuilddep is not built on the target.
    //
    // (This is v1-only for now -- there's a bug in the v2 resolver which we'll
    // need to fix.)
    CargoResolutionCase::new(&["reqbuilddep-weak"])
        .resolver(CargoResolverVersion::V1)
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("reqbuilddep-weak")),
            (json::METADATA_BUILDDEP_REQBUILDDEP,   None),
        ])
        .host_expected(&[
            (json::METADATA_BUILDDEP_REQBUILDDEP,   Some("alloc std")),
        ]),
    // [features]
    // reqbuilddep-weak = ["reqbuilddep?/std"]
    // reqbuilddep = ["dep:reqbuilddep"]
    //
    // Enabling `reqbuilddep` activates the optional normal dependency, and
    // `reqbuilddep?/std` then applies on the target as well.
    CargoResolutionCase::new(&["reqbuilddep-weak", "reqbuilddep"])
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("reqbuilddep reqbuilddep-weak dep:reqbuilddep")),
            (json::METADATA_BUILDDEP_REQBUILDDEP,   Some("alloc std")),
        ])
        .host_expected(&[
            (json::METADATA_BUILDDEP_REQBUILDDEP,   Some("alloc std")),
        ]),
];

#[test]
fn resolution_matches_cargo() {
    let graph = JsonFixture::metadata_builddep().graph();
    for case in CASES {
        case.check(graph, json::METADATA_BUILDDEP_MAIN, "");
    }
}
