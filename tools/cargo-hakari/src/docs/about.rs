// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! About workspace-hack crates, how `cargo hakari` manages them, and how much faster they make
//! builds.
//!
//! # What are workspace-hack crates?
//!
//! Let's say you have a Rust crate `my-crate` with two dependencies:
//!
//! ```toml
//! # my-crate/Cargo.toml
//! [dependencies]
//! foo = "1.0"
//! bar = "2.0"
//! ```
//!
//! Let's say that `foo` and `bar` both depend on `baz`:
//!
//! ```toml
//! # foo-1.0/Cargo.toml
//! [dependencies]
//! baz = { version = "1", features = ["a", "b"] }
//!
//! # bar-2.0/Cargo.toml
//! [dependencies]
//! baz = { version = "1", features = ["b", "c"] }
//! ```
//!
//! What features is `baz` built with?
//!
//! One way to resolve this question might be to build `baz` twice with each requested set of
//! features. But this is likely to cause a combinatorial explosion of crates to build, so Cargo
//! doesn't do that. Instead, [Cargo builds `baz`
//! once](https://doc.rust-lang.org/nightly/cargo/reference/features.html?highlight=feature#feature-unification)
//! with the *union* of the features enabled for the package: `[a, b, c]`.
//!
//! ---
//!
//! **NOTE:** This description elides some details around unifying build and dev-dependencies: for
//! more about this, see the documentation for guppy's
//! [`CargoResolverVersion`](guppy::graph::cargo::CargoResolverVersion).
//!
//! ---
//!
//! Now let's say you're in a workspace, with a second crate `your-crate`:
//!
//! ```toml
//! # your-crate/Cargo.toml
//! [dependencies]
//! baz = { version = "1", features = ["c", "d"] }
//! ```
//!
//! In this situation:
//!
//! | if you build                                 | `baz` is built with |
//! | -------------------------------------------- | ------------------- |
//! | just `my-crate`                              | `a, b, c`           |
//! | just `your-crate`                            | `c, d`              |
//! | `my-crate` and `your-crate` at the same time | `a, b, c, d`        |
//!
//! Even in this simplified scenario, there are three separate ways to build `baz`. For a dependency
//! like [`syn`](https://crates.io/crates/syn) that has [many optional
//! features](https://github.com/dtolnay/syn#optional-features), large workspaces end up with a very
//! large number of possible build configurations.
//!
//! Even worse, the feature set of a package affects everything that depends on it, so `syn` being
//! built with a slightly different feature set than before would cause *every package that directly
//! or transitively depends on `syn` to be rebuilt. For large workspaces, this can result a lot of
//! wasted build time.
//!
//! ---
//!
//! To avoid this problem, many large workspaces contain a `workspace-hack` crate. The purpose of
//! this package is to ensure that dependencies like `syn` are always built with the same feature
//! set no matter which workspace packages are currently being built. This is done by:
//! 1. adding dependencies like `syn` to `workspace-hack` with the full feature set required by any
//!    package in the workspace
//! 2. adding `workspace-hack` as a dependency of every crate in the repository.
//!
//! Some examples of `workspace-hack` packages:
//!
//! * Rust's
//!   [`rustc-workspace-hack`](https://github.com/rust-lang/rust/blob/0bfc45aa859b94cedeffcbd949f9aaad9f3ac8d8/src/tools/rustc-workspace-hack/Cargo.toml)
//! * Firefox's
//!   [`mozilla-central-workspace-hack`](https://hg.mozilla.org/mozilla-central/file/cf6956a5ec8e21896736f96237b1476c9d0aaf45/build/workspace-hack/Cargo.toml)
//! * Oxide's
//!   [`omicron-workspace-hack`](https://github.com/oxidecomputer/omicron/blob/a8176d58352dedf6e8a90fd97de21ec854ee57d9/workspace-hack/Cargo.toml)
//!
//! These packages have historically been maintained by hand, on a best-effort basis.
//!
//! # What can hakari do?
//!
//! Maintaining workspace-hack packages manually can result in:
//! * Missing crates
//! * Missing feature lists for crates
//! * Outdated feature lists for crates
//!
//! All of these can result in longer than optimal build times.
//!
//! `cargo hakari` can automate the maintenance of these packages, greatly reducing the amount of
//! time and effort it takes to maintain these packages.
//!
//! # How does hakari work?
//!
//! `cargo hakari` uses [guppy]'s Cargo build simulations to determine the full set of features that
//! can be built for each package. It then looks for dependencies that are built in more than one
//! way. With this information:
//!
//! * `cargo hakari` constructs a `workspace-hack` package with the union of the feature sets for
//!   each dependency.
//! * `cargo hakari` can also add lines to the `Cargo.toml` files of all workspace crates, to ensure
//!   that the `workspace-hack` package is always included.
//!
//! For more details about the algorithm, see the documentation for the [`hakari`] library.
//!
//! # How much faster do builds get?
//!
//! The amount to which builds get faster depends on the size of the repository. In general, the
//! benefit grows super-linearly with the size of the workspace and the number of crates in it.
//!
//! On moderately large workspaces with several hundred third-party dependencies, a cumulative
//! performance benefit of up to **1.7x** has been seen. Individual commands can be anywhere from
//! **1.1x** to **100x** faster. `cargo check` often benefits more than `cargo build` because
//! expensive linker invocations aren't a factor.
//!
//! ## Benchmarks
//!
//! For a moderately large workspace, here's a chart of cumulative build times across a range of
//! `cargo build` commands, with and without hakari:
//!
//! ![](https://raw.githubusercontent.com/guppy-rs/hakari-on-omicron-perf/refs/heads/main/cumulative.png)
//!
//! The orange line ("Without Hakari") is the default experience provided by Cargo, while the blue
//! line ("With Hakari") is the default experience with `cargo hakari` enabled. The green line
//! ("Hakari without target-host unification") is an advanced option: see
//! [`UnifyTargetHost`](hakari::UnifyTargetHost) for more.
//!
//! For more information including the raw data, see [this
//! repository](https://github.com/guppy-rs/hakari-on-omicron-perf).
//!
//! # Drawbacks
//!
//! * The first build in a workspace might take longer because more dependencies have to be cached.
//!   - This also applies to builds performed after `cargo clean`, or after Rust version upgrades.
//!   - However, in some cases the first build has been observed to be faster.
//!   - In any case, the first build is a relatively small part of overall interactive build times.
//! * Some crates may accidentally start skipping features they really need, because the
//!   workspace-hack turns those features on for them.
//!   - This is not a major issue for repositories that don't release crates to `crates.io`.
//!   - It can also be caught at publish time, or with a periodic CI job that does a build after
//!     running `cargo hakari disable`.
//! * Publishing crates to a registry becomes more complex: see the [publishing
//!   section](crate::publishing) for more about this.
//! * Downstream users that import your crate directly from your repository, rather than from the
//!   registry, are going to import dependencies from the checked in workspace-hack. This can be
//!   avoided by following the instructions in the [`[patch]` directive
//!   section](crate::patch_directive).
