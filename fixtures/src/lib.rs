// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Test fixtures for guppy.

// Referenced so that Cargo's unused_dependencies lint sees the workspace-hack
// dependency as used.
use guppy_workspace_hack as _;

pub mod dep_helpers;
pub mod details;
pub mod json;

use guppy::PackageId;

/// Helper for creating `PackageId` instances in test code.
pub fn package_id(s: impl Into<Box<str>>) -> PackageId {
    PackageId::new(s)
}
