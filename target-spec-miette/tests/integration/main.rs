// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use snapbox::{Data, data::DataFormat};
use target_spec::errors::{
    CustomTripleCreateError, Error as TargetSpecError, RustcVersionVerboseParseError,
};
use target_spec_miette::IntoMietteDiagnostic;

#[test]
fn unavailable_snapshot() {
    // SAFETY: Tests are run under nextest where it's safe to alter the
    // environment.
    unsafe { std::env::set_var("CLICOLOR_FORCE", "1") };

    // Test that the unavailable diagnostic shows properly as a report.
    let report =
        miette::Report::new(CustomTripleCreateError::CustomJsonUnavailable.into_diagnostic());
    // Use the Debug format to get the report ace the fancy displayer would show
    // it.
    let actual = format!("{report:?}");

    let b = snapbox::Assert::new().action_env("SNAPSHOTS");

    // Store SVG and ANSI snapshots. Use the binary representation to ensure
    // that no post-processing of text happens.
    b.eq(
        Data::binary(actual.clone()).coerce_to(DataFormat::TermSvg),
        snapbox::file!["snapshots/unavailable.svg"],
    );
    b.eq(
        Data::binary(actual),
        snapbox::file!["snapshots/unavailable.ansi"],
    );
}

#[test]
fn rustc_version_verbose_missing_host_snapshot() {
    // SAFETY: Tests are run under nextest where it's safe to alter the
    // environment.
    unsafe { std::env::set_var("CLICOLOR_FORCE", "1") };

    // This is realistic `rustc -vV` output that is just missing the `host:` line.
    let output = "\
rustc 1.86.0 (05f9846f8 2025-03-31)
binary: rustc
commit-hash: 05f9846f893b09a1be1fc8560e11b1d4d2f085ef
commit-date: 2025-03-31
release: 1.86.0
LLVM version: 19.1.7
"
    .to_owned();

    let error =
        TargetSpecError::RustcVersionVerboseParse(RustcVersionVerboseParseError::MissingHostLine {
            output,
        });
    let report = miette::Report::new_boxed(error.into_diagnostic());
    let actual = format!("{report:?}");

    let b = snapbox::Assert::new().action_env("SNAPSHOTS");

    b.eq(
        Data::binary(actual.clone()).coerce_to(DataFormat::TermSvg),
        snapbox::file!["snapshots/rustc_version_verbose_missing_host.svg"],
    );
    b.eq(
        Data::binary(actual),
        snapbox::file!["snapshots/rustc_version_verbose_missing_host.ansi"],
    );
}
