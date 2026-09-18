// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Tests for weak dependency features (`dep?/feature`), mostly on a dependency
//! that is required in one declaration and optional in another.
//!
//! The `builddep` fixture's `main` has a build script. The parts of its
//! manifest exercised here are:
//!
//! ```toml
//! [features]
//! platdep-weak = ["platdep?/std"]
//! platdep-slash = ["platdep/std"]
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
//! The expected results were obtained from Cargo 1.98.1 with
//! `cargo build --unit-graph -Z unstable-options`, under both resolver
//! versions 1 and 2. Cases that build dev-dependencies were obtained with
//! `cargo build --tests`.

use crate::feature_helpers::{
    CargoResolutionCase, SeenLink, VisitStatus, WINDOWS, feature_ids, graph_with_patched_json,
    specs,
};
use fixtures::{
    json::{self, JsonFixture},
    package_id,
};
use guppy::{
    PackageId,
    graph::{
        DependencyDirection, PackageGraph,
        cargo::CargoResolverVersion,
        feature::{FeatureId, LinkDeclarations},
    },
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
    CargoResolutionCase::new(&["reqbuilddep-weak"])
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("reqbuilddep-weak")),
            (json::METADATA_BUILDDEP_REQBUILDDEP,   None),
        ])
        .host_expected(&[
            (json::METADATA_BUILDDEP_REQBUILDDEP,   Some("alloc std")),
        ]),
    // The same with the v1 resolver.
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
    CargoResolutionCase::new(&["pmdep-weak"])
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("pmdep-weak")),
            (json::METADATA_BUILDDEP_PMDEP,         None),
        ])
        .host_expected(&[
            (json::METADATA_BUILDDEP_PMDEP,         Some("alloc std")),
        ]),
    // The same with the v1 resolver.
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
    CargoResolutionCase::new(&["reqbuilddep-weak", "targetuser-reqbuilddep"])
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("reqbuilddep-weak targetuser-reqbuilddep")),
            (json::METADATA_BUILDDEP_TARGETUSER,    Some("reqbuilddep dep:reqbuilddep")),
            (json::METADATA_BUILDDEP_REQBUILDDEP,   Some("")),
        ])
        .host_expected(&[
            (json::METADATA_BUILDDEP_REQBUILDDEP,   Some("alloc std")),
        ]),
    // Same for the v1 resolver. v1 unifies host and target features, so the
    // target copy gets `std` too.
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

    // [features]
    // normaldep-weak = ["normaldep?/std"]
    // hostuser-normaldep = ["hostuser/normaldep"]
    //
    // The same as the case above, with host and target swapped. normaldep is
    // now built twice:
    //
    // * on the target, because it is a required normal dependency of `main`.
    // * on the host, because `hostuser` (a build dependency of `main`) turned
    //   on its own optional dependency on it.
    //
    // `main` also has an optional host-side dependency on normaldep, but
    // nothing has turned it on. So `normaldep?/std` only counts for the target
    // copy: the target copy gets `std`, and the host copy gets no features.
    CargoResolutionCase::new(&["normaldep-weak", "hostuser-normaldep"])
        .target_expected(&[
            (json::METADATA_BUILDDEP_MAIN,          Some("normaldep-weak hostuser-normaldep")),
            (json::METADATA_BUILDDEP_NORMALDEP,     Some("alloc std")),
        ])
        .host_expected(&[
            (json::METADATA_BUILDDEP_HOSTUSER,      Some("normaldep dep:normaldep")),
            (json::METADATA_BUILDDEP_NORMALDEP,     Some("")),
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
    // `main` only lists targetuser under `[dependencies]`, without
    // `optional = true`.
    let graph = builddep_graph_with_main_features(&[(
        "targetuser-weak",
        serde_json::json!(["targetuser?/reqbuilddep"]),
    )]);

    for case in NEVER_OPTIONAL_CASES {
        case.check(&graph, json::METADATA_BUILDDEP_MAIN, "");
    }

    // targetuser has no optional declarations, so there is no optional half to
    // hold back. The edge is not weak at all: the visitor sees it once, as an
    // `Unsplit` link like the one for `targetuser/reqbuilddep`.
    let targetuser = package_id(json::METADATA_BUILDDEP_TARGETUSER);
    let visits = weak_edge_visits(
        &graph,
        "targetuser-weak",
        "targetuser-weak",
        &targetuser,
        "reqbuilddep",
    );
    assert_eq!(
        visits,
        [SeenLink {
            declarations: LinkDeclarations::Unsplit,
            normal: VisitStatus::Always,
            build: VisitStatus::Never,
            dev: VisitStatus::Never,
        }],
        "a weak feature on a never-optional dep is visited once"
    );
}

#[test]
fn resolution_matches_cargo() {
    let graph = JsonFixture::metadata_builddep().graph();
    for case in CASES {
        case.check(graph, json::METADATA_BUILDDEP_MAIN, "");
    }
}

// The two halves of `main/normaldep-weak` -> `normaldep/std`: normaldep is
// declared as a required normal dependency and an optional build dependency.
fn normaldep_weak_halves() -> [SeenLink; 2] {
    [
        SeenLink {
            declarations: LinkDeclarations::Required,
            normal: VisitStatus::Always,
            build: VisitStatus::Never,
            dev: VisitStatus::Never,
        },
        SeenLink {
            declarations: LinkDeclarations::Optional,
            normal: VisitStatus::Never,
            build: VisitStatus::Always,
            dev: VisitStatus::Never,
        },
    ]
}

// Returns the builddep fixture with `features` patched into `main`.
//
// `cargo metadata` can produce manifests the fixtures don't cover, so some
// tests need a shape that isn't checked in. Patching the JSON keeps the rest
// of the fixture intact.
fn builddep_graph_with_main_features(features: &[(&str, serde_json::Value)]) -> PackageGraph {
    graph_with_patched_json(JsonFixture::metadata_builddep(), |metadata| {
        let main = metadata["packages"]
            .as_array_mut()
            .expect("packages is an array")
            .iter_mut()
            .find(|package| package["id"] == json::METADATA_BUILDDEP_MAIN)
            .expect("main is in the builddep fixture");
        let main_features = main["features"].as_object_mut().expect("features is a map");
        for (name, value) in features {
            main_features.insert((*name).to_owned(), value.clone());
        }
    })
}

// Resolves `features` on `main` with a visitor that accepts every link, and
// returns the visits of the link from `main/<from_feature>` to
// `<to_package>/<to_feature>`, in order.
fn weak_edge_visits(
    graph: &PackageGraph,
    features: &str,
    from_feature: &str,
    to_package: &PackageId,
    to_feature: &str,
) -> Vec<SeenLink> {
    let main = package_id(json::METADATA_BUILDDEP_MAIN);
    let weak_from = FeatureId::named(&main, from_feature);
    let weak_to = FeatureId::named(to_package, to_feature);

    let mut visits = Vec::new();
    graph
        .feature_graph()
        .query_forward(feature_ids(&main, features))
        .expect("valid feature IDs")
        .resolve_with_fn(|_, link| {
            if link.from().feature_id() == weak_from && link.to().feature_id() == weak_to {
                visits.push(SeenLink::from_link(&link));
            }
            true
        });
    visits
}

#[test]
fn weak_edge_visits_each_declaration_once() {
    // [dependencies]
    // normaldep = { path = "../normaldep" }
    //
    // [build-dependencies]
    // normaldep = { path = "../normaldep", optional = true }
    //
    // [features]
    // normaldep-weak = ["normaldep?/std"]
    //
    // `normaldep?/std` is a single edge in the feature graph, from
    // `main/normaldep-weak` to `normaldep/std`. But normaldep is declared
    // twice, and the weak feature applies separately to each declaration.
    let both_halves = normaldep_weak_halves();
    let cases: &[(&str, &[SeenLink])] = &[
        // normaldep is not activated, so only the required half is visited.
        (
            "normaldep-weak",
            &[SeenLink {
                declarations: LinkDeclarations::Required,
                normal: VisitStatus::Always,
                build: VisitStatus::Never,
                dev: VisitStatus::Never,
            }],
        ),
        // Activated later: required, then optional once the weak buffer is
        // released.
        ("normaldep-weak normaldep", &both_halves),
        // Activated first: same two visits, back to back (in the same order).
        ("normaldep-weak dep:normaldep", &both_halves),
    ];

    let graph = JsonFixture::metadata_builddep().graph();
    let normaldep = package_id(json::METADATA_BUILDDEP_NORMALDEP);

    for (features, expected) in cases {
        let actual = weak_edge_visits(graph, features, "normaldep-weak", &normaldep, "std");
        assert_eq!(
            &actual, expected,
            "for features {features:?}, visits of the weak edge match"
        );
    }
}

// [dependencies]
// devdep = { path = "../devdep", optional = true }
//
// [dev-dependencies]
// devdep = { path = "../devdep" }
//
// [features]
// devdep-weak = ["devdep?/std"]
// devdep = ["dep:devdep"]
//
// devdep's only required declaration is a dev-dependency. So the required half
// of the weak edge is dev-only (normal and build are never enabled), and the
// optional half is not. The union of the two would not be dev-only, which is
// why each half carries its own statuses.
#[test]
fn weak_edge_required_half_can_be_dev_only() {
    let graph = JsonFixture::metadata_builddep().graph();
    let devdep = package_id(json::METADATA_BUILDDEP_DEVDEP);
    let actual = weak_edge_visits(graph, "devdep-weak devdep", "devdep-weak", &devdep, "std");
    assert_eq!(
        actual,
        [
            SeenLink {
                declarations: LinkDeclarations::Required,
                normal: VisitStatus::Never,
                build: VisitStatus::Never,
                dev: VisitStatus::Always,
            },
            SeenLink {
                declarations: LinkDeclarations::Optional,
                normal: VisitStatus::Always,
                build: VisitStatus::Never,
                dev: VisitStatus::Never,
            },
        ],
        "visits of the devdep weak edge match"
    );
}

// [target.'cfg(unix)'.dependencies]
// platdep = { path = "../platdep" }
//
// [target.'cfg(windows)'.dependencies]
// platdep = { path = "../platdep", optional = true }
//
// [features]
// platdep-weak = ["platdep?/std"]
// platdep = ["dep:platdep"]
//
// Both of platdep's declarations are normal dependencies, so the two halves of
// the weak edge are split by platform rather than by section. Each one carries
// the `cfg` of the declarations it covers (Unix for the required half, Windows
// for the optional one) rather than being simply always or never enabled.
#[test]
fn weak_edge_halves_can_split_by_platform() {
    let graph = JsonFixture::metadata_builddep().graph();
    let platdep = package_id(json::METADATA_BUILDDEP_PLATDEP);
    let actual = weak_edge_visits(
        graph,
        "platdep-weak platdep",
        "platdep-weak",
        &platdep,
        "std",
    );
    assert_eq!(
        actual,
        [
            SeenLink {
                declarations: LinkDeclarations::Required,
                normal: specs(&["unix"]),
                build: VisitStatus::Never,
                dev: VisitStatus::Never,
            },
            SeenLink {
                declarations: LinkDeclarations::Optional,
                normal: specs(&["windows"]),
                build: VisitStatus::Never,
                dev: VisitStatus::Never,
            },
        ],
        "visits of the platdep weak edge match"
    );
}

// [dependencies]
// normaldep = { path = "../normaldep" }
//
// [build-dependencies]
// normaldep = { path = "../normaldep", optional = true }
//
// [features]
// normaldep-weak = ["normaldep?/std"]
//
// The visitor sees both halves of the weak edge, required first, in either
// direction. The edge is followed if it accepts at least one of them.
//
// The opposite endpoint can only be reached through this edge, so whether
// it is in the resulting feature set says whether the edge was followed.
#[test]
fn weak_edge_followed_if_either_half_accepted() {
    #[derive(Clone, Copy, Debug)]
    enum Reject {
        Required,
        Optional,
        Both,
    }

    let graph = JsonFixture::metadata_builddep().graph();
    let main = package_id(json::METADATA_BUILDDEP_MAIN);
    let weak_from = FeatureId::named(&main, "normaldep-weak");
    let normaldep = package_id(json::METADATA_BUILDDEP_NORMALDEP);
    let weak_to = FeatureId::named(&normaldep, "std");

    // `dep:normaldep` releases the optional half's buffer in the forward query;
    // without it that half is never offered.
    let forward_initials: Vec<_> = feature_ids(&main, "normaldep-weak dep:normaldep").collect();
    let directions: [(DependencyDirection, &[FeatureId<'_>], FeatureId<'_>); 2] = [
        (DependencyDirection::Forward, &forward_initials, weak_to),
        (DependencyDirection::Reverse, &[weak_to], weak_from),
    ];

    for (reject, expected_present) in [
        (Reject::Required, true),
        (Reject::Optional, true),
        (Reject::Both, false),
    ] {
        for (direction, initials, expected_feature) in directions {
            let mut visits = Vec::new();
            let feature_set = graph
                .feature_graph()
                .query_directed(initials.iter().copied(), direction)
                .expect("valid feature IDs")
                .resolve_with_fn(|_, link| {
                    if link.from().feature_id() != weak_from || link.to().feature_id() != weak_to {
                        return true;
                    }
                    visits.push(SeenLink::from_link(&link));
                    match (reject, link.declarations()) {
                        (Reject::Both, _) => false,
                        (Reject::Required, LinkDeclarations::Required)
                        | (Reject::Optional, LinkDeclarations::Optional) => false,
                        (Reject::Required, LinkDeclarations::Optional)
                        | (Reject::Optional, LinkDeclarations::Required) => true,
                        (Reject::Required | Reject::Optional, LinkDeclarations::Unsplit) => {
                            panic!("weak edge halves are Required or Optional, not Unsplit")
                        }
                    }
                });
            assert_eq!(
                feature_set
                    .contains(expected_feature)
                    .expect("valid feature ID"),
                expected_present,
                "with {reject:?} rejected in {direction:?}, opposite endpoint presence matches"
            );
            assert_eq!(
                visits,
                normaldep_weak_halves(),
                "with {reject:?} rejected in {direction:?}, both halves are visited once, \
                 required first"
            );
        }
    }
}

// An accept-all visitor follows every edge, which is what `resolve` does, so
// the two must agree in reverse.
//
// `hyper_util_7afb1ed` has `regex-automata` with
// `logging = ["aho-corasick?/logging"]` and an optional `aho-corasick`. A
// reverse query from `aho-corasick/logging` reaches no other link between the
// two packages, so a buffered optional half would never be released.
#[test]
fn reverse_accept_all_matches_resolve() {
    for (name, fixture) in JsonFixture::all_fixtures() {
        assert_reverse_accept_all_matches_resolve(name, fixture.graph());
    }
}

fn assert_reverse_accept_all_matches_resolve(name: &str, graph: &PackageGraph) {
    let feature_graph = graph.feature_graph();
    for feature in feature_graph
        .resolve_all()
        .feature_ids(DependencyDirection::Forward)
    {
        let query = feature_graph
            .query_reverse([feature])
            .expect("valid feature ID");
        let visited = query.clone().resolve_with_fn(|_, _| true);
        let resolved = query.resolve();
        // Print only the differences: the sets themselves can be large.
        let missing: Vec<_> = resolved
            .difference(&visited)
            .feature_ids(DependencyDirection::Forward)
            .map(|feature_id| feature_id.to_string())
            .collect();
        let extra: Vec<_> = visited
            .difference(&resolved)
            .feature_ids(DependencyDirection::Forward)
            .map(|feature_id| feature_id.to_string())
            .collect();
        assert!(
            missing.is_empty() && extra.is_empty(),
            "for fixture {name}, an accept-all reverse visitor from {feature} matches \
             resolve (missing: {missing:?}, extra: {extra:?})"
        );
    }
}

#[test]
fn conditional_links_report_declarations() {
    let graph = JsonFixture::metadata_builddep().graph();
    let main = package_id(json::METADATA_BUILDDEP_MAIN);
    let normaldep = package_id(json::METADATA_BUILDDEP_NORMALDEP);
    let optbuilddep = package_id(json::METADATA_BUILDDEP_OPTBUILDDEP);
    let platdep = package_id(json::METADATA_BUILDDEP_PLATDEP);

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
        // `optbuilddep/std` also activates `dep:optbuilddep`, but only through
        // optional declarations of optbuilddep, so that link is `Optional`.
        (
            FeatureId::named(&main, "slash"),
            FeatureId::optional_dependency(&main, "optbuilddep"),
            SeenLink {
                declarations: LinkDeclarations::Optional,
                normal: VisitStatus::Never,
                build: VisitStatus::Always,
                dev: VisitStatus::Never,
            },
        ),
        // platdep-slash = ["platdep/std"]
        //
        // `platdep/std` applies to both declarations of platdep, so the link
        // to platdep's `std` is `Unsplit`.
        (
            FeatureId::named(&main, "platdep-slash"),
            FeatureId::named(&platdep, "std"),
            SeenLink {
                declarations: LinkDeclarations::Unsplit,
                normal: specs(&["unix", "windows"]),
                build: VisitStatus::Never,
                dev: VisitStatus::Never,
            },
        ),
        // The links to `dep:platdep` and the implicit `platdep` feature only
        // cover the optional `cfg(windows)` declaration.
        (
            FeatureId::named(&main, "platdep-slash"),
            FeatureId::optional_dependency(&main, "platdep"),
            SeenLink {
                declarations: LinkDeclarations::Optional,
                normal: specs(&["windows"]),
                build: VisitStatus::Never,
                dev: VisitStatus::Never,
            },
        ),
        (
            FeatureId::named(&main, "platdep-slash"),
            FeatureId::named(&main, "platdep"),
            SeenLink {
                declarations: LinkDeclarations::Optional,
                normal: specs(&["windows"]),
                build: VisitStatus::Never,
                dev: VisitStatus::Never,
            },
        ),
    ];

    let feature_set = graph
        .feature_graph()
        .query_forward(feature_ids(
            &main,
            "normaldep-weak dep:normaldep slash platdep-slash",
        ))
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
