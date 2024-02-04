// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Using a `[patch]` directive.
//!
//! To work effectively, `cargo hakari` requires that all the other crates in your workspace depend
//! on it. This is done by adding a `workspace-hack` dependency to each crate's `Cargo.toml` file, a
//! process that can be automated by running `cargo hakari manage-deps` locally.
//!
//! By default, the `workspace-hack` dependency are added like this:
//!
//! ```toml
//! [dependencies]
//! my-workspace-hack = { path = "../workspace-hack", version = "0.1.0" }
//! ```
//!
//! This is the simplest way to get started with `cargo hakari`. However, it has a significant
//! limitation: if other projects depend on your code as a path or Git dependency, they will pull in
//! all the dependencies from your local workspace-hack crate. This is typically undesirable.
//!
//! To avoid this outcome, hakari allows you to use a `[patch]` directive. This page outlines the
//! steps to do so.
//!
//! ## 1. Publish a stub crate to crates.io
//!
//! If you haven't already done so, follow the instructions in the [publishing
//! section](crate::publishing) section to publish a uniquely-named stub crate to crates.io. This
//! needs to only be done once.
//!
//! ## 2. Refer to the stub crate by default
//!
//! After this step, the workspace-hack dependency will be updated to refer to the stub crate on
//! crates.io. (We will restore use of the real workspace-hack in step 3.)
//!
//! To do this, add a `workspace-hack-line-style` option to `.config/hakari.toml`. There are two
//! options, both of which are equivalent from hakari's perspective.
//!
//! ### A. `"version-only"`
//!
//! This option is the closest to the default.
//!
//! Update `hakari.toml` with:
//!
//! ```toml
//! workspace-hack-line-style = "version-only"
//! ```
//!
//! Then, run:
//!
//! ```sh
//! cargo hakari remove-deps
//! cargo hakari manage-deps
//! ```
//!
//! This will cause the workspace-hack lines to be updated to be similar to:
//!
//! ```toml
//! [dependencies]
//! my-workspace-hack = { version = "0.1.0" }
//! ```
//!
//! ### B. `"workspace-dotted"`
//!
//! This option lets you specify the path to the workspace-hack crate, once, in the root
//! `Cargo.toml`. You may prefer this if you've standardized on this format in your workspace.
//!  
//! Update `hakari.toml` with:
//!
//! ```toml
//! workspace-hack-line-style = "workspace-dotted"
//! ```
//!
//! Also, add the following to your root `Cargo.toml`:
//!
//! ```toml
//! [workspace.dependencies]
//! my-workspace-hack = "0.1.0"  # or another version number if you've changed it
//! ```
//!
//! Then, run
//!
//! ```sh
//! cargo hakari remove-deps
//! cargo hakari manage-deps
//! ```
//!
//! This will cause the workspace-hack lines to be updated to be similar to:
//!
//! ```toml
//! [dependencies]
//! my-workspace-hack.workspace = true
//! ```
//!
//! ## 3. Add a `[patch]` directive to the root `Cargo.toml`
//!
//! To the workspace's root `Cargo.toml`, add a `[patch]` directive that points to the local
//! dependency:
//!
//! ```toml
//! [patch.crates-io.my-workspace-hack]
//! path = "workspace-hack"
//! ```
//!
//! This ensures that while building within the workspace, the real workspace-hack is used. When
//! building outside of the workspace, such as via a Git or path dependency, the `[patch]` directive
//! is inactive, and the stub crate from crates.io is used.
//!
//! # Example
//!
//! The guppy workspace itself uses a `[patch]` directive with `"workspace-dotted"`. Here's [the
//! root `Cargo.toml`](https://github.com/guppy-rs/guppy/blob/main/Cargo.toml), and a [crate
//! `Cargo.toml`](https://github.com/guppy-rs/guppy/blob/main/guppy-summaries/Cargo.toml).
