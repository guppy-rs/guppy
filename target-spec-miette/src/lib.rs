// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Integrate [target-spec](https://crates.io/crates/target-spec) errors with [miette](https://docs.rs/miette).
//!
//! This crate has implementations of `Diagnostic` for the various kinds of errors that target-spec
//! produces. This can be used to pretty-print errors returned by target-spec.
//!
//! ## Minimum supported Rust version
//!
//! The minimum supported Rust version (MSRV) is **Rust 1.73**. While this crate is in pre-release
//! status (0.x), The MSRV may be bumped in patch releases.

#![warn(missing_docs)]
#![forbid(unsafe_code)]
#![cfg_attr(doc_cfg, feature(doc_cfg, doc_auto_cfg))]

use miette::{Diagnostic, LabeledSpan, SourceCode};
use std::{error::Error as StdError, fmt};
use target_spec::errors::{
    Error as TargetSpecError, ExpressionParseError, PlainStringParseError, TripleParseError,
};

/// Extension trait that converts errors into a [`miette::Diagnostic`].
pub trait IntoMietteDiagnostic {
    /// The `Diagnostic` type that `self` will be converted to.
    type IntoDiagnostic;

    /// Converts the underlying error into [`Self::IntoDiagnostic`].
    ///
    /// This can be used to pretty-print errors returned by target-spec.
    fn into_diagnostic(self) -> Self::IntoDiagnostic;
}

impl IntoMietteDiagnostic for TargetSpecError {
    type IntoDiagnostic = Box<dyn Diagnostic + Send + Sync + 'static>;

    fn into_diagnostic(self) -> Self::IntoDiagnostic {
        match self {
            Self::InvalidExpression(error) => Box::new(error.into_diagnostic()),
            Self::InvalidTargetSpecString(error) => Box::new(error.into_diagnostic()),
            Self::UnknownPlatformTriple(error) => Box::new(error.into_diagnostic()),
            other => Box::<dyn Diagnostic + Send + Sync + 'static>::from(other.to_string()),
        }
    }
}

/// A wrapper around [`ExpressionParseError`] that implements [`Diagnostic`].
#[derive(Clone, PartialEq, Eq)]
pub struct ExpressionParseDiagnostic(ExpressionParseError);

impl ExpressionParseDiagnostic {
    /// Creates a new `ExpressionParseDiagnostic`.
    pub fn new(error: ExpressionParseError) -> Self {
        Self(error)
    }
}

impl fmt::Debug for ExpressionParseDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl fmt::Display for ExpressionParseDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl StdError for ExpressionParseDiagnostic {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.0.source()
    }
}

impl Diagnostic for ExpressionParseDiagnostic {
    fn source_code(&self) -> Option<&dyn SourceCode> {
        Some(&self.0.input)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        let label = LabeledSpan::new_with_span(Some(self.0.kind.to_string()), self.0.span.clone());
        Some(Box::new(std::iter::once(label)))
    }
}

impl IntoMietteDiagnostic for ExpressionParseError {
    type IntoDiagnostic = ExpressionParseDiagnostic;

    fn into_diagnostic(self) -> Self::IntoDiagnostic {
        ExpressionParseDiagnostic::new(self)
    }
}

/// A wrapper around [`TripleParseError`] that implements [`Diagnostic`].
#[derive(Clone, PartialEq, Eq)]
pub struct TripleParseDiagnostic {
    error: TripleParseError,
    // Need to store this separately because &str can't be cast to &dyn SourceCode.
    triple_str: String,
}

impl TripleParseDiagnostic {
    /// Creates a new `ExpressionParseDiagnostic`.
    pub fn new(error: TripleParseError) -> Self {
        let triple_str = error.triple_str().to_owned();
        Self { error, triple_str }
    }
}

impl fmt::Debug for TripleParseDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.error, f)
    }
}

impl fmt::Display for TripleParseDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.error, f)
    }
}

impl StdError for TripleParseDiagnostic {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.error.source()
    }
}

impl Diagnostic for TripleParseDiagnostic {
    fn source_code(&self) -> Option<&dyn SourceCode> {
        Some(&self.triple_str as &dyn SourceCode)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        let label = LabeledSpan::new_with_span(
            Some(
                self.error
                    .source()
                    .expect("TripleParseError always returns a source")
                    .to_string(),
            ),
            (0, self.triple_str.len()),
        );
        Some(Box::new(std::iter::once(label)))
    }
}

impl IntoMietteDiagnostic for TripleParseError {
    type IntoDiagnostic = TripleParseDiagnostic;

    fn into_diagnostic(self) -> Self::IntoDiagnostic {
        TripleParseDiagnostic::new(self)
    }
}

/// A wrapper around [`PlainStringParseError`] that implements [`Diagnostic`].
#[derive(Clone, PartialEq, Eq)]
pub struct PlainStringParseDiagnostic {
    error: PlainStringParseError,
    // Need to store this separately because &str can't be cast to &dyn SourceCode.
    input: String,
}

impl PlainStringParseDiagnostic {
    /// Creates a new `ExpressionParseDiagnostic`.
    pub fn new(error: PlainStringParseError) -> Self {
        let input = error.input.clone();
        Self { error, input }
    }
}

impl fmt::Debug for PlainStringParseDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.error, f)
    }
}

impl fmt::Display for PlainStringParseDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The full error message duplicates information produced by the diagnostic, so keep it
        // short.
        f.write_str("invalid triple identifier")
    }
}

impl StdError for PlainStringParseDiagnostic {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.error.source()
    }
}

impl Diagnostic for PlainStringParseDiagnostic {
    fn source_code(&self) -> Option<&dyn SourceCode> {
        Some(&self.input as &dyn SourceCode)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        let label = LabeledSpan::new_with_span(
            Some("character must be alphanumeric, -, _ or .".to_owned()),
            self.error.span(),
        );
        Some(Box::new(std::iter::once(label)))
    }
}

impl IntoMietteDiagnostic for PlainStringParseError {
    type IntoDiagnostic = PlainStringParseDiagnostic;

    fn into_diagnostic(self) -> Self::IntoDiagnostic {
        PlainStringParseDiagnostic::new(self)
    }
}
