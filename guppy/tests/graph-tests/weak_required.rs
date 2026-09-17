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
//! pmdep-weak = ["pmdep?/std"]
//! devdep-weak = ["devdep?/std"]
//! hostuser-normaldep = ["hostuser/normaldep"]
//! targetuser-reqbuilddep = ["targetuser/reqbuilddep"]
//!
//! [dependencies]
//! normaldep = { path = "../normaldep" }
//! reqbuilddep = { path = "../reqbuilddep", optional = true }
//! targetuser = { path = "../targetuser" }
//! devdep = { path = "../devdep", optional = true }
//!
//! [dev-dependencies]
//! devdep = { path = "../devdep" }
//!
//! [build-dependencies]
//! normaldep = { path = "../normaldep", optional = true }
//! reqbuilddep = { path = "../reqbuilddep" }
//! hostuser = { path = "../hostuser" }
//!
//! [target.'cfg(unix)'.dependencies]
//! platdep = { path = "../platdep" }
//! pmdep = { path = "../pmdep" }
//!
//! [target.'cfg(windows)'.dependencies]
//! platdep = { path = "../platdep", optional = true }
//! pmdep = { path = "../pmdep", optional = true }
//! ```
//!
//! `platdep`, `normaldep`, `reqbuilddep`, `pmdep` and `devdep` each have
//! features `alloc = []` and `std = ["alloc"]`. None of these optional
//! dependencies are referred to with `dep:`, so each has an implicit feature of
//! the same name.
//!
//! `pmdep` is a proc macro. When it is built, it is built for the host and
//! never for the target, even though it is listed under `[dependencies]`. On
//! Windows it is optional, so it may not be built at all.
//!
//! `hostuser` and `targetuser` are helper crates. Each one has a single
//! optional dependency, with an implicit feature of the same name:
//!
//! ```toml
//! # hostuser, a required build dependency of main
//! [dependencies]
//! normaldep = { path = "../normaldep", optional = true }
//!
//! # targetuser, a required normal dependency of main
//! [dependencies]
//! reqbuilddep = { path = "../reqbuilddep", optional = true }
//! ```
//!
//! The helpers let a test build a second copy of a crate without turning on
//! `main`'s optional dependency on it:
//!
//! * `main` always needs `normaldep` on the target, and optionally needs it on
//!   the host. Enabling `hostuser-normaldep` makes `hostuser` build
//!   `normaldep` on the host, while `main`'s optional host-side dependency
//!   stays off.
//! * `main` always needs `reqbuilddep` on the host, and optionally needs it on
//!   the target. Enabling `targetuser-reqbuilddep` makes `targetuser` build
//!   `reqbuilddep` on the target, while `main`'s optional target-side
//!   dependency stays off.
//!
//! In both cases, the copy that `main` didn't ask for must not get `std` from
//! `main`'s weak feature.
//!
//! TODO: only the `targetuser` half is tested here. The `hostuser-normaldep`
//! case fails today: a required declaration releases the weak buffer, so the
//! host copy of `normaldep` gets `std`. The next commit fixes that and adds the
//! case.
//!
//! The expected results were obtained from Cargo 1.98.1 with
//! `cargo build --unit-graph -Z unstable-options`, under both resolver
//! versions 1 and 2. Cases that build dev-dependencies were obtained with
//! `cargo build --tests`.

use crate::feature_helpers::{CargoResolutionCase, WINDOWS, feature_ids};
use fixtures::{
    json::{self, JsonFixture},
    package_id,
};
use guppy::{
    graph::{
        DependencyDirection,
        cargo::CargoResolverVersion,
        feature::{ConditionalLink, FeatureId, LinkDeclarations},
    },
    platform::PlatformStatus,
};

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
    // The same with the v1 resolver.
    CargoResolutionCase::new(&[])
        .resolver(CargoResolverVersion::V1)
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
    // The same with the v1 resolver.
    CargoResolutionCase::new(&["platdep-weak"])
        .resolver(CargoResolverVersion::V1)
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
    // The same with the v1 resolver.
    CargoResolutionCase::new(&["platdep-weak", "platdep"])
        .resolver(CargoResolverVersion::V1)
        .target_platform(WINDOWS)
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("platdep platdep-weak dep:platdep")),
            (json::METADATA_BUILDDEP_PLATDEP,       Some("alloc std")),
        ]),
    // On Linux, the `platdep` feature still activates `dep:platdep`, but the
    // optional declaration is `cfg(windows)`-only, so it adds nothing to the
    // build. platdep gets `std` through its required declaration, as it does
    // without the `platdep` feature.
    CargoResolutionCase::new(&["platdep-weak", "platdep"])
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("platdep platdep-weak dep:platdep")),
            (json::METADATA_BUILDDEP_PLATDEP,       Some("alloc std")),
        ]),
    // The same with the v1 resolver.
    CargoResolutionCase::new(&["platdep-weak", "platdep"])
        .resolver(CargoResolverVersion::V1)
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
    // The same with the v1 resolver.
    CargoResolutionCase::new(&["normaldep-weak", "normaldep"])
        .resolver(CargoResolverVersion::V1)
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("normaldep normaldep-weak dep:normaldep")),
            (json::METADATA_BUILDDEP_NORMALDEP,     Some("alloc std")),
        ])
        .host_expected(&[
            (json::METADATA_BUILDDEP_NORMALDEP,     Some("alloc std")),
        ]),

    // [features]
    // normaldep-weak = ["normaldep?/std"]
    //
    // The same as the case above, but with `dep:normaldep` passed in directly
    // as an initial feature, and not reached through the `normaldep` feature.
    // This means that the optional build dependency is already activated by
    // the time `normaldep?/std` is looked at. In the case above, it is
    // activated afterwards.
    //
    // Cargo has no equivalent to this: `--features dep:normaldep` is rejected
    // on the command line. But guppy accepts `dep:` features as initials, so
    // the expected results here can't be checked against Cargo. They are the
    // results of the case above, minus `normaldep` in `main`'s features.
    CargoResolutionCase::new(&["normaldep-weak", "dep:normaldep"])
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("normaldep-weak dep:normaldep")),
            (json::METADATA_BUILDDEP_NORMALDEP,     Some("alloc std")),
        ])
        .host_expected(&[
            (json::METADATA_BUILDDEP_NORMALDEP,     Some("alloc std")),
        ]),
    // The same with the v1 resolver.
    CargoResolutionCase::new(&["normaldep-weak", "dep:normaldep"])
        .resolver(CargoResolverVersion::V1)
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("normaldep-weak dep:normaldep")),
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
    // The same with the v1 resolver.
    CargoResolutionCase::new(&["reqbuilddep-weak", "reqbuilddep"])
        .resolver(CargoResolverVersion::V1)
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("reqbuilddep reqbuilddep-weak dep:reqbuilddep")),
            (json::METADATA_BUILDDEP_REQBUILDDEP,   Some("alloc std")),
        ])
        .host_expected(&[
            (json::METADATA_BUILDDEP_REQBUILDDEP,   Some("alloc std")),
        ]),

    // [features]
    // pmdep-weak = ["pmdep?/std"]
    //
    // pmdep is a proc macro, so it is built for the host and never for the
    // target. On Unix it is a required dependency, so `pmdep?/std` turns on
    // `std` for the host build. A weak feature never turns on an optional
    // dependency, so `main` doesn't get `dep:pmdep`.
    //
    // (This is v1 only for now -- the v2 case is currently buggy.)
    CargoResolutionCase::new(&["pmdep-weak"])
        .resolver(CargoResolverVersion::V1)
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("pmdep-weak")),
            (json::METADATA_BUILDDEP_PMDEP,         None),
        ])
        .host_expected(&[
            (json::METADATA_BUILDDEP_PMDEP,         Some("alloc std")),
        ]),
    // On Windows, pmdep is optional and nothing turns it on, so it isn't built
    // at all.
    CargoResolutionCase::new(&["pmdep-weak"])
        .target_platform(WINDOWS)
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("pmdep-weak")),
            (json::METADATA_BUILDDEP_PMDEP,         None),
        ])
        .host_expected(&[
            (json::METADATA_BUILDDEP_PMDEP,         None),
        ]),
    // The same with the v1 resolver.
    CargoResolutionCase::new(&["pmdep-weak"])
        .resolver(CargoResolverVersion::V1)
        .target_platform(WINDOWS)
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("pmdep-weak")),
            (json::METADATA_BUILDDEP_PMDEP,         None),
        ])
        .host_expected(&[
            (json::METADATA_BUILDDEP_PMDEP,         None),
        ]),
    // [features]
    // pmdep-weak = ["pmdep?/std"]
    // pmdep = ["dep:pmdep"]
    //
    // On Windows, the `pmdep` feature turns on the optional dependency. Now
    // that pmdep is being built, `pmdep?/std` takes effect. pmdep is still a
    // proc macro, so `std` shows up on the host.
    CargoResolutionCase::new(&["pmdep-weak", "pmdep"])
        .target_platform(WINDOWS)
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("pmdep pmdep-weak dep:pmdep")),
            (json::METADATA_BUILDDEP_PMDEP,         None),
        ])
        .host_expected(&[
            (json::METADATA_BUILDDEP_PMDEP,         Some("alloc std")),
        ]),
    // The same with the v1 resolver.
    CargoResolutionCase::new(&["pmdep-weak", "pmdep"])
        .resolver(CargoResolverVersion::V1)
        .target_platform(WINDOWS)
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("pmdep pmdep-weak dep:pmdep")),
            (json::METADATA_BUILDDEP_PMDEP,         None),
        ])
        .host_expected(&[
            (json::METADATA_BUILDDEP_PMDEP,         Some("alloc std")),
        ]),

    // [features]
    // reqbuilddep-weak = ["reqbuilddep?/std"]
    // targetuser-reqbuilddep = ["targetuser/reqbuilddep"]
    //
    // reqbuilddep is now built twice:
    //
    // * on the host, because it is a required build dependency of `main`.
    // * on the target, because `targetuser` turned on its own optional
    //   dependency on it.
    //
    // `main` also has an optional target-side dependency on reqbuilddep, but
    // nothing has turned it on. So `reqbuilddep?/std` only counts for the host
    // copy: the host copy gets `std`, and the target copy gets no features.
    //
    // (This is v1 only for now -- the v2 case is currently buggy.)
    CargoResolutionCase::new(&["reqbuilddep-weak", "targetuser-reqbuilddep"])
        .resolver(CargoResolverVersion::V1)
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("reqbuilddep-weak targetuser-reqbuilddep")),
            (json::METADATA_BUILDDEP_TARGETUSER,    Some("reqbuilddep dep:reqbuilddep")),
            (json::METADATA_BUILDDEP_REQBUILDDEP,   Some("alloc std")),
        ])
        .host_expected(&[
            (json::METADATA_BUILDDEP_REQBUILDDEP,   Some("alloc std")),
        ]),

    // With dev-dependencies built and nothing enabled, devdep is built through
    // its required dev declaration.
    CargoResolutionCase::new(&[])
        .include_dev()
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("")),
            (json::METADATA_BUILDDEP_DEVDEP,        Some("")),
        ])
        .host_expected(&[
            (json::METADATA_BUILDDEP_DEVDEP,        None),
        ]),
    // The same with the v1 resolver.
    CargoResolutionCase::new(&[])
        .resolver(CargoResolverVersion::V1)
        .include_dev()
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("")),
            (json::METADATA_BUILDDEP_DEVDEP,        Some("")),
        ])
        .host_expected(&[
            (json::METADATA_BUILDDEP_DEVDEP,        None),
        ]),
    // [features]
    // devdep-weak = ["devdep?/std"]
    //
    // devdep's only required declaration is a dev-dependency. Without
    // dev-dependencies, nothing builds devdep, and `devdep?/std` doesn't
    // activate the optional normal declaration.
    CargoResolutionCase::new(&["devdep-weak"])
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("devdep-weak")),
            (json::METADATA_BUILDDEP_DEVDEP,        None),
        ])
        .host_expected(&[
            (json::METADATA_BUILDDEP_DEVDEP,        None),
        ]),
    // The same with the v1 resolver.
    CargoResolutionCase::new(&["devdep-weak"])
        .resolver(CargoResolverVersion::V1)
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("devdep-weak")),
            (json::METADATA_BUILDDEP_DEVDEP,        None),
        ])
        .host_expected(&[
            (json::METADATA_BUILDDEP_DEVDEP,        None),
        ]),
    // With dev-dependencies built, the required dev declaration is active, so
    // `devdep?/std` applies to it. `main` still doesn't get `dep:devdep`.
    CargoResolutionCase::new(&["devdep-weak"])
        .include_dev()
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("devdep-weak")),
            (json::METADATA_BUILDDEP_DEVDEP,        Some("alloc std")),
        ])
        .host_expected(&[
            (json::METADATA_BUILDDEP_DEVDEP,        None),
        ]),
    // The same with the v1 resolver.
    CargoResolutionCase::new(&["devdep-weak"])
        .resolver(CargoResolverVersion::V1)
        .include_dev()
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("devdep-weak")),
            (json::METADATA_BUILDDEP_DEVDEP,        Some("alloc std")),
        ])
        .host_expected(&[
            (json::METADATA_BUILDDEP_DEVDEP,        None),
        ]),
    // [features]
    // devdep-weak = ["devdep?/std"]
    // devdep = ["dep:devdep"]
    //
    // Enabling `devdep` activates the optional normal declaration, and
    // `devdep?/std` then applies without dev-dependencies as well.
    CargoResolutionCase::new(&["devdep-weak", "devdep"])
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("devdep devdep-weak dep:devdep")),
            (json::METADATA_BUILDDEP_DEVDEP,        Some("alloc std")),
        ])
        .host_expected(&[
            (json::METADATA_BUILDDEP_DEVDEP,        None),
        ]),
    // The same with the v1 resolver.
    CargoResolutionCase::new(&["devdep-weak", "devdep"])
        .resolver(CargoResolverVersion::V1)
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("devdep devdep-weak dep:devdep")),
            (json::METADATA_BUILDDEP_DEVDEP,        Some("alloc std")),
        ])
        .host_expected(&[
            (json::METADATA_BUILDDEP_DEVDEP,        None),
        ]),
];

// [dependencies]
// targetuser = { path = "../targetuser" }
//
// [features]
// targetuser-weak = ["targetuser?/reqbuilddep"]
//
// This is a weak dependency feature on a dependency that is never optional.
// Cargo rejects this while parsing the manifest:
//
//     feature `targetuser-weak` includes `targetuser?/reqbuilddep` with a `?`,
//     but `targetuser` is not an optional dependency
//
// But guppy accepts this, (logically) treating it as the same as
// `targetuser/reqbuilddep`.
//
// TODO: This should probably be a FeatureGraphWarning.
#[rustfmt::skip]
static NEVER_OPTIONAL_CASES: &[CargoResolutionCase] = &[
    CargoResolutionCase::new(&["targetuser-weak"])
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("targetuser-weak")),
            (json::METADATA_BUILDDEP_TARGETUSER,    Some("reqbuilddep dep:reqbuilddep")),
            (json::METADATA_BUILDDEP_REQBUILDDEP,   Some("")),
        ]),
    CargoResolutionCase::new(&["targetuser-weak"])
        .resolver(CargoResolverVersion::V1)
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("targetuser-weak")),
            (json::METADATA_BUILDDEP_TARGETUSER,    Some("reqbuilddep dep:reqbuilddep")),
            (json::METADATA_BUILDDEP_REQBUILDDEP,   Some("")),
        ]),
];

// `cargo metadata` never produces the manifest in NEVER_OPTIONAL_CASES. But
// this case isn't rejected by guppy, so we still have to produce an answer
// without panicking. Check that by patching the feature into the builddep
// fixture's JSON.
#[test]
fn weak_feature_on_never_optional_dep() {
    let mut metadata: serde_json::Value =
        serde_json::from_str(JsonFixture::metadata_builddep().json())
            .expect("builddep fixture is valid JSON");
    let main = metadata["packages"]
        .as_array_mut()
        .expect("packages is an array")
        .iter_mut()
        .find(|package| package["id"] == json::METADATA_BUILDDEP_MAIN)
        .expect("main is in the builddep fixture");
    // `main` only lists targetuser under `[dependencies]`, without
    // `optional = true`.
    main["features"]
        .as_object_mut()
        .expect("features is a map")
        .insert(
            "targetuser-weak".to_owned(),
            serde_json::json!(["targetuser?/reqbuilddep"]),
        );
    let graph = guppy::graph::PackageGraph::from_json(metadata.to_string())
        .expect("patched metadata is valid");

    for case in NEVER_OPTIONAL_CASES {
        case.check(&graph, json::METADATA_BUILDDEP_MAIN, "");
    }
}

#[test]
fn resolution_matches_cargo() {
    let graph = JsonFixture::metadata_builddep().graph();
    for case in CASES {
        case.check(graph, json::METADATA_BUILDDEP_MAIN, "");
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum VisitStatus {
    Always,
    Never,
    // Enabled on platforms matching these target specs.
    Specs(Vec<String>),
}

impl VisitStatus {
    fn new(status: PlatformStatus<'_>) -> Self {
        match status {
            PlatformStatus::Always => VisitStatus::Always,
            PlatformStatus::Never => VisitStatus::Never,
            PlatformStatus::PlatformDependent { eval } => VisitStatus::Specs(
                eval.target_specs()
                    .iter()
                    .map(|spec| spec.to_string())
                    .collect(),
            ),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SeenLink {
    declarations: LinkDeclarations,
    normal: VisitStatus,
    build: VisitStatus,
    dev: VisitStatus,
}

impl SeenLink {
    fn from_link(link: &ConditionalLink<'_>) -> Self {
        Self {
            declarations: link.declarations(),
            normal: VisitStatus::new(link.normal()),
            build: VisitStatus::new(link.build()),
            dev: VisitStatus::new(link.dev()),
        }
    }
}

#[test]
fn conditional_links_report_declarations() {
    let graph = JsonFixture::metadata_builddep().graph();
    let main = package_id(json::METADATA_BUILDDEP_MAIN);
    let normaldep = package_id(json::METADATA_BUILDDEP_NORMALDEP);
    let optbuilddep = package_id(json::METADATA_BUILDDEP_OPTBUILDDEP);

    // The weak edge is one unified link here, not two halves unlike
    // weak_edge_visits_each_declaration_once above.
    let expected = [
        (
            FeatureId::named(&main, "normaldep-weak"),
            FeatureId::named(&normaldep, "std"),
            SeenLink {
                declarations: LinkDeclarations::Unsplit,
                normal: VisitStatus::Always,
                build: VisitStatus::Always,
                dev: VisitStatus::Never,
            },
        ),
        (
            FeatureId::base(&main),
            FeatureId::base(&normaldep),
            SeenLink {
                declarations: LinkDeclarations::Required,
                normal: VisitStatus::Always,
                build: VisitStatus::Never,
                dev: VisitStatus::Never,
            },
        ),
        (
            FeatureId::optional_dependency(&main, "normaldep"),
            FeatureId::base(&normaldep),
            SeenLink {
                declarations: LinkDeclarations::Optional,
                normal: VisitStatus::Never,
                build: VisitStatus::Always,
                dev: VisitStatus::Never,
            },
        ),
        // slash = ["optbuilddep/std"]
        //
        // Without the `?`, the link is never split by declaration, so it is
        // `Unsplit`. optbuilddep is only an optional build dependency.
        (
            FeatureId::named(&main, "slash"),
            FeatureId::named(&optbuilddep, "std"),
            SeenLink {
                declarations: LinkDeclarations::Unsplit,
                normal: VisitStatus::Never,
                build: VisitStatus::Always,
                dev: VisitStatus::Never,
            },
        ),
    ];

    let feature_set = graph
        .feature_graph()
        .query_forward(feature_ids(&main, "normaldep-weak dep:normaldep slash"))
        .expect("valid feature IDs")
        .resolve();
    for (from, to, expected) in expected {
        let actual: Vec<_> = feature_set
            .conditional_links(DependencyDirection::Forward)
            .filter(|link| link.from().feature_id() == from && link.to().feature_id() == to)
            .map(|link| SeenLink::from_link(&link))
            .collect();
        assert_eq!(
            actual,
            [expected],
            "exactly one conditional link from {from} to {to}"
        );
    }
}
