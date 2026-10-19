// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Tests for `CargoOptions::set_include_build_deps_without_build_script`.
//!
//! See the option's documentation for how Cargo treats these dependencies,
//! and `build_deps_no_script` for the `build-deps-no-script` fixture used
//! here. With the option off, `memchr` is left out as `cargo build` would.
//! With it on, `memchr` is included as `cargo tree -e normal,build` would.
//! `libc` has `extra_traits` either way.
//!
//! The expected `libc` features were obtained from Cargo 1.101.0-nightly with
//! `cargo build --unit-graph -Z unstable-options`. Resolver version 2 unless
//! noted otherwise.

use crate::feature_helpers::{CargoResolutionCase, WINDOWS};
use fixtures::json::{self, JsonFixture};
use guppy::graph::cargo::CargoResolverVersion;

#[rustfmt::skip]
static CASES: &[CargoResolutionCase] = &[
    // The default leaves main's build dependencies out: memchr is not built.
    // libc is, for with-script's build script, and already has the
    // extra_traits that main requested, as Cargo compiles it.
    CargoResolutionCase::new(&[])
        .target_expected(&[
            (json::METADATA_BUILD_DEPS_NO_SCRIPT_MAIN,        Some("")),
            (json::METADATA_BUILD_DEPS_NO_SCRIPT_WITH_SCRIPT, Some("")),
            (json::METADATA_BUILD_DEPS_NO_SCRIPT_LIBC,        None),
            (json::METADATA_BUILD_DEPS_NO_SCRIPT_MEMCHR,      None),
        ])
        .host_expected(&[
            (json::METADATA_BUILD_DEPS_NO_SCRIPT_LIBC,        Some("extra_traits")),
            (json::METADATA_BUILD_DEPS_NO_SCRIPT_MEMCHR,      None),
        ]),
    // What the feature resolver sees: memchr is activated on the host even
    // though Cargo never compiles it. libc is unchanged.
    CargoResolutionCase::new(&[])
        .include_build_deps_without_build_script()
        .target_expected(&[
            (json::METADATA_BUILD_DEPS_NO_SCRIPT_MAIN,        Some("")),
            (json::METADATA_BUILD_DEPS_NO_SCRIPT_WITH_SCRIPT, Some("")),
            (json::METADATA_BUILD_DEPS_NO_SCRIPT_LIBC,        None),
            (json::METADATA_BUILD_DEPS_NO_SCRIPT_MEMCHR,      None),
        ])
        .host_expected(&[
            (json::METADATA_BUILD_DEPS_NO_SCRIPT_LIBC,        Some("extra_traits")),
            (json::METADATA_BUILD_DEPS_NO_SCRIPT_MEMCHR,      Some("")),
        ]),
    // Build dependencies are evaluated for the host whatever the target is.
    CargoResolutionCase::new(&[])
        .include_build_deps_without_build_script()
        .target_platform(WINDOWS)
        .host_expected(&[
            (json::METADATA_BUILD_DEPS_NO_SCRIPT_LIBC,        Some("extra_traits")),
            (json::METADATA_BUILD_DEPS_NO_SCRIPT_MEMCHR,      Some("")),
        ]),
    // The check happens when packages are assigned to a platform, after
    // features are resolved, so it is the same under version 1 of the feature
    // resolver.
    CargoResolutionCase::new(&[])
        .resolver(CargoResolverVersion::V1)
        .host_expected(&[
            (json::METADATA_BUILD_DEPS_NO_SCRIPT_LIBC,        Some("extra_traits")),
            (json::METADATA_BUILD_DEPS_NO_SCRIPT_MEMCHR,      None),
        ]),
    CargoResolutionCase::new(&[])
        .resolver(CargoResolverVersion::V1)
        .include_build_deps_without_build_script()
        .host_expected(&[
            (json::METADATA_BUILD_DEPS_NO_SCRIPT_LIBC,        Some("extra_traits")),
            (json::METADATA_BUILD_DEPS_NO_SCRIPT_MEMCHR,      Some("")),
        ]),
];

#[test]
fn resolution_matches_cargo() {
    let graph = JsonFixture::metadata_build_deps_no_script().graph();
    for case in CASES {
        case.check(graph, json::METADATA_BUILD_DEPS_NO_SCRIPT_MAIN, "");
    }
}
