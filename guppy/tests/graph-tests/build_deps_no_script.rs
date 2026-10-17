// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Tests for build dependencies declared by a package without a build script.
//!
//! Cargo resolves features for such dependencies but never builds them: the
//! feature resolver does not look for a build script, while the unit graph
//! only reaches build dependencies through a build script unit. The two halves
//! are visible separately when another package *with* a build script shares
//! one of the dependencies.
//!
//! The `build-deps-no-script` fixture has two workspace members:
//!
//! ```toml
//! # main/Cargo.toml -- no build.rs.
//! [dependencies]
//! with-script = { path = "../with-script" }
//!
//! [build-dependencies]
//! libc = { version = "0.2", default-features = false, features = ["extra_traits"] }
//! memchr = { version = "2", default-features = false }
//!
//! # with-script/Cargo.toml -- has a build.rs.
//! [build-dependencies]
//! libc = { version = "0.2", default-features = false }
//! ```
//!
//! Building `main` compiles libc once, for with-script's build script, with
//! `extra_traits` unified in from main's declaration. memchr is never
//! compiled.

use crate::feature_helpers::CargoResolutionCase;
use fixtures::json::{self, JsonFixture};
use guppy::graph::cargo::CargoResolverVersion;

#[rustfmt::skip]
static FROM_MAIN: &[CargoResolutionCase] = &[
    // libc reaches the host through with-script's build script, and picks up
    // `extra_traits` from main's declaration. memchr has no build script to
    // reach the host through, so it is not built anywhere.
    CargoResolutionCase::new(&[])
        .target_expected(&[
            (json::METADATA_BUILD_DEPS_NO_SCRIPT_MAIN,          Some("")),
            (json::METADATA_BUILD_DEPS_NO_SCRIPT_WITH_SCRIPT,   Some("")),
            (json::METADATA_BUILD_DEPS_NO_SCRIPT_LIBC,          None),
            (json::METADATA_BUILD_DEPS_NO_SCRIPT_MEMCHR,        None),
        ])
        .host_expected(&[
            (json::METADATA_BUILD_DEPS_NO_SCRIPT_MAIN,          None),
            (json::METADATA_BUILD_DEPS_NO_SCRIPT_WITH_SCRIPT,   None),
            (json::METADATA_BUILD_DEPS_NO_SCRIPT_LIBC,          Some("extra_traits")),
            (json::METADATA_BUILD_DEPS_NO_SCRIPT_MEMCHR,        None),
        ]),
    // The v1 resolver unifies features across host and target, but the set of
    // packages built is the same.
    CargoResolutionCase::new(&[])
        .resolver(CargoResolverVersion::V1)
        .target_expected(&[
            (json::METADATA_BUILD_DEPS_NO_SCRIPT_LIBC,          None),
            (json::METADATA_BUILD_DEPS_NO_SCRIPT_MEMCHR,        None),
        ])
        .host_expected(&[
            (json::METADATA_BUILD_DEPS_NO_SCRIPT_LIBC,          Some("extra_traits")),
            (json::METADATA_BUILD_DEPS_NO_SCRIPT_MEMCHR,        None),
        ]),
];

#[rustfmt::skip]
static FROM_WITH_SCRIPT: &[CargoResolutionCase] = &[
    // Without main in the picture, nothing asks for `extra_traits`.
    CargoResolutionCase::new(&[])
        .host_expected(&[
            (json::METADATA_BUILD_DEPS_NO_SCRIPT_LIBC,          Some("")),
            (json::METADATA_BUILD_DEPS_NO_SCRIPT_MEMCHR,        None),
        ]),
];

#[test]
fn resolution_matches_cargo() {
    let graph = JsonFixture::metadata_build_deps_no_script().graph();
    for case in FROM_MAIN {
        case.check(graph, json::METADATA_BUILD_DEPS_NO_SCRIPT_MAIN, "");
    }
    for case in FROM_WITH_SCRIPT {
        case.check(graph, json::METADATA_BUILD_DEPS_NO_SCRIPT_WITH_SCRIPT, "");
    }
}
