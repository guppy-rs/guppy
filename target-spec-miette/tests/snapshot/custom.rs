// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Tests for custom platforms with miette.
//!
//! These tests live here because they depend on target-spec with the custom
//! feature enabled, as well as target-spec-miette.

use datatest_stable::Utf8Path;
use insta::internals::SettingsBindDropGuard;
use target_spec::TargetFeatures;
use target_spec_miette::IntoMietteDiagnostic;

pub(crate) fn test_invalid_custom_json(
    path: &Utf8Path,
    contents: String,
) -> datatest_stable::Result<()> {
    let (_guard, insta_prefix) = bind_insta_settings(path);

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

/// Binds insta settings for a test, and returns the prefix to use for snapshots.
fn bind_insta_settings(path: &Utf8Path) -> (SettingsBindDropGuard, &str) {
    let mut settings = insta::Settings::clone_current();
    // Make insta suitable for datatest-stable use.
    settings.set_input_file(path);
    settings.set_snapshot_path("custom-invalid/output");
    settings.set_prepend_module_to_snapshot(false);

    let guard = settings.bind_to_scope();
    let insta_prefix = path.file_name().unwrap();

    (guard, insta_prefix)
}
