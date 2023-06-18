// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Errors returned by `target-spec`.

use std::{borrow::Cow, error, fmt};

/// An error that happened during `target-spec` parsing or evaluation.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// A `cfg()` expression was invalid and could not be parsed.
    InvalidExpression(ExpressionParseError),
    /// The provided target triple (in the position that a `cfg()` expression would be) was unknown.
    UnknownTargetTriple(TripleParseError),
    /// The provided platform triple was unknown.
    UnknownPlatformTriple(TripleParseError),
    /// The provided `cfg()` expression parsed correctly, but it had an unknown predicate.
    ///
    /// This is no longer used, but is kept for backwards compatibility.
    #[deprecated(since = "1.1.0", note = "this variant is no longer in use")]
    #[doc(hidden)]
    UnknownPredicate(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::InvalidExpression(_) => write!(f, "invalid cfg() expression"),
            Error::UnknownTargetTriple(_) => write!(f, "unknown target triple"),
            Error::UnknownPlatformTriple(_) => {
                write!(f, "unknown platform triple")
            }
            #[allow(deprecated)]
            Error::UnknownPredicate(pred) => {
                write!(f, "cfg() expression has unknown predicate: {}", pred)
            }
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Error::InvalidExpression(err) => Some(err),
            Error::UnknownTargetTriple(err) => Some(err),
            Error::UnknownPlatformTriple(err) => Some(err),
            #[allow(deprecated)]
            Error::UnknownPredicate(_) => None,
        }
    }
}

// Note: ExpressionParseError is a duplicate of cfg_expr::error::ParseError, and is copied here
// because we don't want to expose that in a stable (1.0+) API.

/// An error returned in case a `TargetExpression` cannot be parsed.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ExpressionParseError {
    /// The string we tried to parse.
    pub input: String,

    /// The range of characters in the original string that resulted
    /// in this error.
    pub span: std::ops::Range<usize>,

    /// The kind of error that occurred.
    pub kind: ExpressionParseErrorKind,
}

impl ExpressionParseError {
    pub(crate) fn new(input: &str, error: cfg_expr::ParseError) -> Self {
        // The error returned by cfg_expr::ParseError does not include the leading 'cfg('. Use the
        // original input and add 4 which is the length of 'cfg('.
        let span = if input.starts_with("cfg(") && input.ends_with(')') {
            (error.span.start + 4)..(error.span.end + 4)
        } else {
            error.span
        };
        Self {
            input: input.to_owned(),
            span,
            kind: ExpressionParseErrorKind::from_cfg_expr(error.reason),
        }
    }
}

impl fmt::Display for ExpressionParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "error parsing cfg() expression")
    }
}

impl error::Error for ExpressionParseError {}

/// The kind of [`ExpressionParseError`] that occurred.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExpressionParseErrorKind {
    /// not() takes exactly 1 predicate, unlike all() and any()
    InvalidNot(usize),
    /// The characters are not valid in an cfg expression
    InvalidCharacters,
    /// An opening parens was unmatched with a closing parens
    UnclosedParens,
    /// A closing parens was unmatched with an opening parens
    UnopenedParens,
    /// An opening quotes was unmatched with a closing quotes
    UnclosedQuotes,
    /// A closing quotes was unmatched with an opening quotes
    UnopenedQuotes,
    /// The expression does not contain any valid terms
    Empty,
    /// Found an unexpected term, which wasn't one of the expected terms that
    /// is listed
    Unexpected {
        /// The list of expected terms.
        expected: &'static [&'static str],
    },
    /// Failed to parse an integer value
    InvalidInteger,
    /// The root cfg() may only contain a single predicate
    MultipleRootPredicates,
    /// A `target_has_atomic` predicate didn't correctly parse.
    InvalidHasAtomic,
    /// An element was not part of the builtin information in rustc
    UnknownBuiltin,
}

impl ExpressionParseErrorKind {
    fn from_cfg_expr(reason: cfg_expr::error::Reason) -> Self {
        use cfg_expr::error::Reason::*;

        match reason {
            InvalidCharacters => Self::InvalidCharacters,
            UnclosedParens => Self::UnclosedParens,
            UnopenedParens => Self::UnopenedParens,
            UnclosedQuotes => Self::UnclosedQuotes,
            UnopenedQuotes => Self::UnopenedQuotes,
            Empty => Self::Empty,
            Unexpected(expected) => Self::Unexpected { expected },
            InvalidNot(np) => Self::InvalidNot(np),
            InvalidInteger => Self::InvalidInteger,
            MultipleRootPredicates => Self::MultipleRootPredicates,
            InvalidHasAtomic => Self::InvalidHasAtomic,
            UnknownBuiltin => Self::UnknownBuiltin,
        }
    }
}

impl fmt::Display for ExpressionParseErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ExpressionParseErrorKind::*;

        match self {
            InvalidCharacters => f.write_str("invalid character(s)"),
            UnclosedParens => f.write_str("unclosed parens"),
            UnopenedParens => f.write_str("unopened parens"),
            UnclosedQuotes => f.write_str("unclosed quotes"),
            UnopenedQuotes => f.write_str("unopened quotes"),
            Empty => f.write_str("empty expression"),
            Unexpected { expected } => {
                if expected.len() > 1 {
                    f.write_str("expected one of ")?;

                    for (i, exp) in expected.iter().enumerate() {
                        f.write_fmt(format_args!("{}`{exp}`", if i > 0 { ", " } else { "" }))?;
                    }
                    f.write_str(" here")
                } else if !expected.is_empty() {
                    f.write_fmt(format_args!("expected a `{}` here", expected[0]))
                } else {
                    f.write_str("the term was not expected here")
                }
            }
            InvalidNot(np) => f.write_fmt(format_args!("not() takes 1 predicate, found {np}")),
            InvalidInteger => f.write_str("invalid integer"),
            MultipleRootPredicates => f.write_str("multiple root predicates"),
            InvalidHasAtomic => f.write_str("expected integer or \"ptr\""),
            UnknownBuiltin => f.write_str("unknown built-in"),
        }
    }
}

/// An error returned while parsing a single target.
///
/// This is caused by a triple not being understood by either `cfg-expr` or `target-lexicon`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TripleParseError {
    triple_str: Cow<'static, str>,
    lexicon_err: cfg_expr::target_lexicon::ParseError,
}

impl TripleParseError {
    pub(crate) fn new(
        triple_str: Cow<'static, str>,
        lexicon_err: cfg_expr::target_lexicon::ParseError,
    ) -> Self {
        Self {
            triple_str,
            lexicon_err,
        }
    }

    /// Returns the triple string that could not be parsed.
    pub fn triple_str(&self) -> &str {
        &self.triple_str
    }
}

impl fmt::Display for TripleParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown triple string: {}", self.triple_str)
    }
}

impl error::Error for TripleParseError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        Some(&self.lexicon_err)
    }
}

#[cfg(feature = "custom")]
mod custom_errors {
    use std::{error, fmt};

    /// An error returned while creating a custom platform.
    #[derive(Debug)]
    #[non_exhaustive]
    pub enum CustomPlatformError {
        /// An error occurred while deserializing serde data.
        Deserialize {
            /// The specified triple.
            triple: String,

            /// The deserialization error that occurred.
            error: serde_json::Error,
        },
    }

    impl fmt::Display for CustomPlatformError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::Deserialize { triple, .. } => {
                    write!(f, "error deserializing custom target JSON for `{triple}`")
                }
            }
        }
    }

    impl error::Error for CustomPlatformError {
        fn source(&self) -> Option<&(dyn error::Error + 'static)> {
            match self {
                Self::Deserialize { error, .. } => Some(error),
            }
        }
    }
}

#[cfg(feature = "custom")]
pub use custom_errors::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TargetExpression;
    use test_case::test_case;

    #[test_case("cfg()", 4..4; "empty expression results in span inside cfg")]
    #[test_case("target_os = \"macos", 12..18; "unclosed quote specified without cfg")]
    fn test_expression_parse_error_span(input: &str, expected_span: std::ops::Range<usize>) {
        let err = match TargetExpression::new(input).unwrap_err() {
            Error::InvalidExpression(err) => err,
            other => {
                panic!("unexpected error type {other:?}");
            }
        };
        assert_eq!(err.span, expected_span);
    }
}
