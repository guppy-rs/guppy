// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use target_spec::errors::CustomTripleCreateError;
use target_spec_miette::IntoMietteDiagnostic;

#[test]
fn unavailable_snapshot() {
    // Test that the unavailable diagnostic shows properly as a report.
    let report = miette::Report::new(CustomTripleCreateError::Unavailable.into_diagnostic());
    insta::assert_snapshot!(format!("{report:?}"));
}
