// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::helpers::bind_insta_settings;
use datatest_stable::Utf8Path;
use target_spec_miette::IntoMietteDiagnostic;

pub(crate) fn expr_invalid(path: &Utf8Path, contents: String) -> datatest_stable::Result<()> {
    std::env::set_var("CLICOLOR_FORCE", "1");

    let (_guard, insta_prefix) =
        bind_insta_settings(path, "../datatest-snapshot/snapshots/expr-invalid");

    let error = target_spec::TargetSpec::new(contents.trim_end().to_owned())
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
