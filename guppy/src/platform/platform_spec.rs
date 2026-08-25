// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

#[allow(unused_imports)]
use crate::platform::EnabledTernary;
use crate::{errors::TargetSpecError, platform::Platform};
use std::sync::Arc;

/// A specifier for a set of platforms.
///
/// Some uses of `guppy` care about one or more specific platforms, and others
/// care about queries against the intersection of all hypothetical platforms,
/// or against a union of any of them. `PlatformSpec` represents this notion.
///
/// # Ordering
///
/// For any dependency status, the results of queries against these specs are
/// ordered (by [`EnabledTernary`]'s `Ord` impl, where `Disabled < Unknown <
/// Enabled`) as:
///
/// ```text
/// Platforms([]) <= Always <= Platforms(non-empty) <= Any
/// ```
///
/// `Platforms` over every known platform is still not the same as `Any`, since
/// the latter also covers platforms that `guppy` does not know about.
///
/// `PlatformSpec` does not currently support expressions, but it might in the future, using an
/// [SMT solver](https://en.wikipedia.org/wiki/Satisfiability_modulo_theories).
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum PlatformSpec {
    /// The intersection of all platforms.
    ///
    /// Dependency queries performed against this variant will return [`EnabledTernary::Enabled`] if
    /// and only if a dependency is not platform-dependent. They can never return
    /// [`EnabledTernary::Unknown`].
    ///
    /// This variant does not currently understand expressions that always evaluate to true
    /// (tautologies), like `cfg(any(unix, not(unix)))` or `cfg(all())`. In the future, an SMT
    /// solver would be able to handle such expressions.
    Always,

    /// The union of a set of individual platforms.
    ///
    /// Dependency queries performed against this variant will return
    /// [`EnabledTernary::Enabled`] if and only if a dependency is enabled on at
    /// least one platform. They may also return [`EnabledTernary::Unknown`] if
    /// the dependency isn't definitely enabled on any platform, but the status
    /// is unknown on at least one platform (due to target features being
    /// unknown).
    ///
    /// If the list is empty, every query against it returns
    /// [`EnabledTernary::Disabled`], even for dependencies that are not
    /// platform-dependent.
    ///
    /// Queries against this variant obey the following laws, where `|` is the
    /// K3 OR on [`EnabledTernary`]:
    ///
    /// * The order of platforms doesn't matter, and duplicates don't change
    ///   the result.
    /// * `Platforms(a ++ b)` produces the same result as `Platforms(a) |
    ///   Platforms(b)`.
    /// * For platform-dependent statuses, `Platforms([p])` produces the same
    ///   result as [`PlatformEval::eval`](crate::platform::PlatformEval::eval)
    ///   against `p`.
    Platforms(Vec<Arc<Platform>>),

    /// The union of all platforms.
    ///
    /// Dependency queries performed against this variant will return [`EnabledTernary::Enabled`] if
    /// a dependency is enabled on any platform.
    ///
    /// This variant does not currently understand expressions that always evaluate to false
    /// (contradictions), like `cfg(all(unix, not(unix)))` or `cfg(any())`. In the future, an SMT
    /// solver would be able to handle such expressions.
    Any,
}

impl PlatformSpec {
    /// Returns a `PlatformSpec` corresponding to the target platform, as
    /// determined at build time.
    ///
    /// Returns an error if the build target was unknown to the version of
    /// `target-spec` in use.
    pub fn build_target() -> Result<Self, TargetSpecError> {
        Ok(PlatformSpec::from(Platform::build_target()?))
    }

    /// Returns a `PlatformSpec` that matches any of the given platforms.
    ///
    /// An empty iterator produces `Platforms([])`, in which case no
    /// dependencies are enabled. Callers that build the list by filtering a
    /// larger set should be careful about this case.
    pub fn platforms<I, P>(platforms: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<Arc<Platform>>,
    {
        PlatformSpec::Platforms(
            platforms
                .into_iter()
                .map(|platform| platform.into())
                .collect(),
        )
    }
}

impl<T: Into<Arc<Platform>>> From<T> for PlatformSpec {
    #[inline]
    fn from(platform: T) -> Self {
        PlatformSpec::Platforms(vec![platform.into()])
    }
}
