// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Tests for custom platforms with miette.
//!
//! These tests live here because they depend on target-spec with the custom
//! feature enabled, as well as target-spec-miette.

use crate::helpers::bind_insta_settings;
use datatest_stable::Utf8Path;
use target_spec::TargetFeatures;
use target_spec_miette::IntoMietteDiagnostic;

pub(crate) fn custom_invalid(path: &Utf8Path, contents: String) -> datatest_stable::Result<()> {
    let (_guard, insta_prefix) =
        bind_insta_settings(path, "../datatest-snapshot/snapshots/custom-invalid");

    let error = target_spec::Platform::new_custom("my-target", &contents, TargetFeatures::none())
        .expect_err("expected input to fail");

    let diagnostic = error.into_diagnostic();
    insta::assert_snapshot!(
        format!("{insta_prefix}-display"),
        // This displays fancy output. Note the use of assert_snapshot, not
        // assert_debug_snapshot, since the latter uses the pretty-printed Debug
        // (which doesn't do what you would expect with miette/anyhow etc).
        format!("{:?}", miette::Report::new_boxed(diagnostic)),
    );

    Ok(())
}
