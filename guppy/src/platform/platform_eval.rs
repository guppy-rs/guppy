// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::platform::{Platform, PlatformSpec};
use std::ops::{BitAnd, BitOr};
use target_spec::TargetSpec;

/// The status of a dependency or feature, which is possibly platform-dependent.
///
/// This is a sub-status of [`EnabledStatus`](crate::graph::EnabledStatus).
#[derive(Copy, Clone, Debug)]
pub enum PlatformStatus<'g> {
    /// This dependency or feature is never enabled on any platforms.
    Never,
    /// This dependency or feature is always enabled on all platforms.
    Always,
    /// The status is platform-dependent.
    PlatformDependent {
        /// An evaluator to run queries against.
        eval: PlatformEval<'g>,
    },
}

assert_covariant!(PlatformStatus);

impl<'g> PlatformStatus<'g> {
    pub(crate) fn new(specs: &'g PlatformStatusImpl) -> Self {
        match specs {
            PlatformStatusImpl::Always => PlatformStatus::Always,
            PlatformStatusImpl::Specs(specs) => {
                if specs.is_empty() {
                    PlatformStatus::Never
                } else {
                    PlatformStatus::PlatformDependent {
                        eval: PlatformEval { specs },
                    }
                }
            }
        }
    }

    /// Returns true if this dependency is always enabled on all platforms.
    pub fn is_always(&self) -> bool {
        match self {
            PlatformStatus::Always => true,
            PlatformStatus::PlatformDependent { .. } | PlatformStatus::Never => false,
        }
    }

    /// Returns true if this dependency is never enabled on any platform.
    pub fn is_never(&self) -> bool {
        match self {
            PlatformStatus::Never => true,
            PlatformStatus::PlatformDependent { .. } | PlatformStatus::Always => false,
        }
    }

    /// Returns true if this dependency is possibly enabled on any platform.
    pub fn is_present(&self) -> bool {
        !self.is_never()
    }

    /// Evaluates whether this dependency is enabled on the given platform spec.
    ///
    /// Returns `Unknown` if the result was unknown, which may happen if
    /// evaluating against [`PlatformSpec::Platforms`] and the target features
    /// of one of its platforms are unknown.
    pub fn enabled_on(&self, platform_spec: &PlatformSpec) -> EnabledTernary {
        match platform_spec {
            PlatformSpec::Any => match self {
                PlatformStatus::Always | PlatformStatus::PlatformDependent { .. } => {
                    EnabledTernary::Enabled
                }
                PlatformStatus::Never => EnabledTernary::Disabled,
            },
            PlatformSpec::Always => match self {
                PlatformStatus::Always => EnabledTernary::Enabled,
                PlatformStatus::Never | PlatformStatus::PlatformDependent { .. } => {
                    EnabledTernary::Disabled
                }
            },
            PlatformSpec::Platforms(platforms) => EnabledTernary::or_all(
                platforms
                    .iter()
                    .map(|platform| self.enabled_on_platform(platform)),
            ),
        }
    }

    fn enabled_on_platform(&self, platform: &Platform) -> EnabledTernary {
        match self {
            PlatformStatus::Always => EnabledTernary::Enabled,
            PlatformStatus::Never => EnabledTernary::Disabled,
            PlatformStatus::PlatformDependent { eval } => eval.eval(platform),
        }
    }
}

/// Whether a dependency or feature is enabled on a specific platform.
///
/// This is a ternary or [three-valued logic](https://en.wikipedia.org/wiki/Three-valued_logic)
/// because the result may be unknown in some situations.
///
/// Returned by the methods on `EnabledStatus`, `PlatformStatus`, and `PlatformEval`.
//
// Variant order is important: the derived `Ord` (`Disabled < Unknown <
// Enabled`) is the K3 lattice order, and the `BitAnd`/`BitOr` impls below are
// defined as `min`/`max` over it. `PlatformSpec`'s documented ordering of
// query results also relies on it.
#[derive(Copy, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum EnabledTernary {
    /// The dependency is disabled on this platform.
    Disabled,
    /// The status of this dependency is unknown on this platform.
    ///
    /// This may happen if evaluation involves unknown target features. Notably,
    /// this will not be returned when evaluating only against
    /// [`Platform::build_target()`], since the target features for the build
    /// target platform are determined at compile time.
    Unknown,
    /// The dependency is enabled on this platform.
    Enabled,
}

impl EnabledTernary {
    fn new(x: Option<bool>) -> Self {
        match x {
            Some(false) => EnabledTernary::Disabled,
            None => EnabledTernary::Unknown,
            Some(true) => EnabledTernary::Enabled,
        }
    }

    /// The K3 OR of all items in the iterator.
    fn or_all(iter: impl IntoIterator<Item = EnabledTernary>) -> Self {
        let mut res = EnabledTernary::Disabled;
        for item in iter {
            // Short-circuit evaluation if possible.
            if item == EnabledTernary::Enabled {
                return EnabledTernary::Enabled;
            }
            res = res | item;
        }
        res
    }

    /// Returns true if the status is known (either enabled or disabled).
    pub fn is_known(self) -> bool {
        match self {
            EnabledTernary::Disabled | EnabledTernary::Enabled => true,
            EnabledTernary::Unknown => false,
        }
    }
}

/// AND operation in Kleene K3 logic.
impl BitAnd for EnabledTernary {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        self.min(rhs)
    }
}

/// OR operation in Kleene K3 logic.
impl BitOr for EnabledTernary {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        self.max(rhs)
    }
}

/// An evaluator for platform-specific dependencies.
///
/// This represents a collection of platform specifications, of the sort `cfg(unix)`.
#[derive(Copy, Clone, Debug)]
pub struct PlatformEval<'g> {
    specs: &'g [TargetSpec],
}

assert_covariant!(PlatformEval);

impl<'g> PlatformEval<'g> {
    /// Runs this evaluator against the given platform.
    pub fn eval(&self, platform: &Platform) -> EnabledTernary {
        EnabledTernary::or_all(
            self.specs
                .iter()
                .map(|spec| EnabledTernary::new(spec.eval(platform))),
        )
    }

    /// Returns the [`TargetSpec`] instances backing this evaluator.
    ///
    /// The result of [`PlatformEval::eval`] against a platform is a logical OR
    /// of the results of evaluating the platform against each target spec.
    pub fn target_specs(&self) -> &'g [TargetSpec] {
        self.specs
    }
}

#[derive(Clone, Debug)]
pub(crate) enum PlatformStatusImpl {
    Always,
    // Empty vector means never.
    Specs(Vec<TargetSpec>),
}

impl PlatformStatusImpl {
    /// Returns true if this is an empty predicate (i.e. will never match).
    pub(crate) fn is_never(&self) -> bool {
        match self {
            PlatformStatusImpl::Always => false,
            PlatformStatusImpl::Specs(specs) => specs.is_empty(),
        }
    }

    pub(crate) fn extend(&mut self, other: &PlatformStatusImpl) {
        // &mut *self is a reborrow to allow *self to work below.
        match (&mut *self, other) {
            (PlatformStatusImpl::Always, _) => {
                // Always stays the same since it means all specs are included.
            }
            (PlatformStatusImpl::Specs(_), PlatformStatusImpl::Always) => {
                // Mark self as Always.
                *self = PlatformStatusImpl::Always;
            }
            (PlatformStatusImpl::Specs(specs), PlatformStatusImpl::Specs(other)) => {
                specs.extend_from_slice(other.as_slice());
            }
        }
    }

    pub(crate) fn add_spec(&mut self, spec: Option<&TargetSpec>) {
        // &mut *self is a reborrow to allow *self to work below.
        match (&mut *self, spec) {
            (PlatformStatusImpl::Always, _) => {
                // Always stays the same since it means all specs are included.
            }
            (PlatformStatusImpl::Specs(_), None) => {
                // Mark self as Always.
                *self = PlatformStatusImpl::Always;
            }
            (PlatformStatusImpl::Specs(specs), Some(spec)) => {
                specs.push(spec.clone());
            }
        }
    }
}

impl Default for PlatformStatusImpl {
    fn default() -> Self {
        // Empty vector means never.
        PlatformStatusImpl::Specs(vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::TargetFeatures;
    use std::sync::Arc;

    fn platform(triple: &'static str, target_features: TargetFeatures) -> Arc<Platform> {
        Arc::new(Platform::new(triple, target_features).expect("triple is known"))
    }

    fn specs(specs: &[&'static str]) -> PlatformStatusImpl {
        PlatformStatusImpl::Specs(
            specs
                .iter()
                .map(|spec| TargetSpec::new(*spec).expect("spec is valid"))
                .collect(),
        )
    }

    fn linux() -> Arc<Platform> {
        platform("x86_64-unknown-linux-gnu", TargetFeatures::Unknown)
    }

    fn windows() -> Arc<Platform> {
        platform("x86_64-pc-windows-msvc", TargetFeatures::Unknown)
    }

    fn windows_sse() -> Arc<Platform> {
        platform(
            "x86_64-pc-windows-msvc",
            TargetFeatures::features(["sse"].iter().copied()),
        )
    }

    #[test]
    fn platforms_is_a_union() {
        let status_impl = specs(&["cfg(windows)"]);
        let status = PlatformStatus::new(&status_impl);

        assert_eq!(
            status.enabled_on(&PlatformSpec::Platforms(vec![linux(), windows()])),
            EnabledTernary::Enabled,
            "enabled on the union of linux and windows",
        );
        assert_eq!(
            status.enabled_on(&PlatformSpec::Platforms(vec![linux()])),
            EnabledTernary::Disabled,
            "disabled on the union of just linux",
        );
        assert_eq!(
            status.enabled_on(&PlatformSpec::Platforms(vec![windows()])),
            EnabledTernary::Enabled,
            "enabled on the union of just windows",
        );
    }

    #[test]
    fn platforms_empty_is_disabled() {
        let status_impl = specs(&["cfg(windows)"]);
        let status = PlatformStatus::new(&status_impl);
        assert_eq!(
            status.enabled_on(&PlatformSpec::Platforms(vec![])),
            EnabledTernary::Disabled,
            "the empty union is vacuously disabled",
        );

        // Always is "enabled on every platform", but there are no platforms
        // here, so the union is still disabled.
        let always = PlatformStatus::new(&PlatformStatusImpl::Always);
        assert_eq!(
            always.enabled_on(&PlatformSpec::Platforms(vec![])),
            EnabledTernary::Disabled,
            "an always-enabled status is disabled on the empty union",
        );
        assert_eq!(
            always.enabled_on(&PlatformSpec::Platforms(vec![linux()])),
            EnabledTernary::Enabled,
            "an always-enabled status is enabled on a non-empty union",
        );

        let never_impl = specs(&[]);
        let never = PlatformStatus::new(&never_impl);
        assert_eq!(
            never.enabled_on(&PlatformSpec::Platforms(vec![linux(), windows()])),
            EnabledTernary::Disabled,
            "a never-enabled status stays disabled",
        );
    }

    #[test]
    fn platforms_unknown_follows_k3_or() {
        let status_impl = specs(&["cfg(all(windows, target_feature = \"sse\"))"]);
        let status = PlatformStatus::new(&status_impl);

        assert_eq!(
            status.enabled_on(&PlatformSpec::from(windows())),
            EnabledTernary::Unknown,
            "unknown target features make the status unknown",
        );
        assert_eq!(
            status.enabled_on(&PlatformSpec::Platforms(vec![linux(), windows()])),
            EnabledTernary::Unknown,
            "disabled | unknown is unknown",
        );
        assert_eq!(
            status.enabled_on(&PlatformSpec::Platforms(vec![windows(), windows_sse()])),
            EnabledTernary::Enabled,
            "unknown | enabled is enabled",
        );
    }

    #[test]
    fn k3_truth_tables() {
        use EnabledTernary::*;

        // This is a direct expression of the K3 truth tables, independent of
        // the derived Ord that BitAnd/BitOr use.
        let and_table = [
            (Disabled, Disabled, Disabled),
            (Disabled, Unknown, Disabled),
            (Disabled, Enabled, Disabled),
            (Unknown, Disabled, Disabled),
            (Unknown, Unknown, Unknown),
            (Unknown, Enabled, Unknown),
            (Enabled, Disabled, Disabled),
            (Enabled, Unknown, Unknown),
            (Enabled, Enabled, Enabled),
        ];
        for (a, b, expected) in and_table {
            assert_eq!(a & b, expected, "{a:?} & {b:?}");
        }

        let or_table = [
            (Disabled, Disabled, Disabled),
            (Disabled, Unknown, Unknown),
            (Disabled, Enabled, Enabled),
            (Unknown, Disabled, Unknown),
            (Unknown, Unknown, Unknown),
            (Unknown, Enabled, Enabled),
            (Enabled, Disabled, Enabled),
            (Enabled, Unknown, Enabled),
            (Enabled, Enabled, Enabled),
        ];
        for (a, b, expected) in or_table {
            assert_eq!(a | b, expected, "{a:?} | {b:?}");
        }

        assert_eq!(EnabledTernary::or_all([]), Disabled, "empty OR is Disabled");
        assert_eq!(
            EnabledTernary::or_all([Disabled, Unknown, Disabled]),
            Unknown,
            "OR of Disabled and Unknown is Unknown",
        );
        assert_eq!(
            EnabledTernary::or_all([Unknown, Enabled, Disabled]),
            Enabled,
            "OR with any Enabled is Enabled",
        );
    }
}

#[cfg(all(test, feature = "proptest1"))]
mod proptests {
    use super::*;
    use crate::platform::TargetFeatures;
    use proptest::{
        collection::{SizeRange, vec},
        prelude::*,
        sample::select,
    };
    use std::sync::Arc;

    /// Target specs chosen so that, across the generated platforms, evaluation
    /// produces every `EnabledTernary` value.
    fn target_spec_strategy() -> impl Strategy<Value = TargetSpec> {
        static SPECS: &[&str] = &[
            "cfg(windows)",
            "cfg(unix)",
            "cfg(not(windows))",
            "cfg(target_feature = \"sse\")",
            "cfg(all(unix, target_feature = \"sse\"))",
            "cfg(any(target_os = \"macos\", target_feature = \"avx\"))",
            "x86_64-unknown-linux-gnu",
        ];
        select(SPECS).prop_map(|spec| TargetSpec::new(spec).expect("spec is valid"))
    }

    fn status_impl_strategy() -> impl Strategy<Value = PlatformStatusImpl> {
        prop_oneof![
            1 => Just(PlatformStatusImpl::Always),
            // An empty vector is never.
            4 => vec(target_spec_strategy(), 0..4).prop_map(PlatformStatusImpl::Specs),
        ]
    }

    fn platforms_strategy(size: impl Into<SizeRange>) -> impl Strategy<Value = Vec<Arc<Platform>>> {
        vec(
            Platform::strategy(any::<TargetFeatures>()).prop_map(Arc::new),
            size,
        )
    }

    fn enabled_on(
        status_impl: &PlatformStatusImpl,
        platforms: Vec<Arc<Platform>>,
    ) -> EnabledTernary {
        PlatformStatus::new(status_impl).enabled_on(&PlatformSpec::Platforms(platforms))
    }

    proptest! {
        #[test]
        fn platforms_is_the_k3_or_of_its_elements(
            status_impl in status_impl_strategy(),
            platforms in platforms_strategy(0..4),
        ) {
            let expected = EnabledTernary::or_all(
                platforms
                    .iter()
                    .map(|platform| enabled_on(&status_impl, vec![platform.clone()])),
            );
            prop_assert_eq!(enabled_on(&status_impl, platforms), expected);
        }

        #[test]
        fn platforms_append_is_or(
            status_impl in status_impl_strategy(),
            a in platforms_strategy(0..3),
            b in platforms_strategy(0..3),
        ) {
            let combined: Vec<_> = a.iter().chain(&b).cloned().collect();
            let expected = enabled_on(&status_impl, a) | enabled_on(&status_impl, b);
            prop_assert_eq!(enabled_on(&status_impl, combined), expected);
        }

        #[test]
        fn platforms_order_and_duplicates_do_not_matter(
            status_impl in status_impl_strategy(),
            (platforms, shuffled) in platforms_strategy(0..4).prop_flat_map(|platforms| {
                (Just(platforms.clone()), Just(platforms).prop_shuffle())
            }),
        ) {
            let expected = enabled_on(&status_impl, platforms.clone());
            prop_assert_eq!(enabled_on(&status_impl, shuffled), expected, "order");
            let doubled: Vec<_> = platforms.iter().chain(&platforms).cloned().collect();
            prop_assert_eq!(enabled_on(&status_impl, doubled), expected, "duplicates");
        }

        #[test]
        fn single_platform_matches_direct_eval(
            status_impl in status_impl_strategy(),
            platform in Platform::strategy(any::<TargetFeatures>()),
        ) {
            let status = PlatformStatus::new(&status_impl);
            let expected = match status {
                PlatformStatus::Always => EnabledTernary::Enabled,
                PlatformStatus::Never => EnabledTernary::Disabled,
                PlatformStatus::PlatformDependent { eval } => eval.eval(&platform),
            };
            prop_assert_eq!(status.enabled_on(&PlatformSpec::from(platform)), expected);
        }

        #[test]
        fn specs_are_ordered(
            status_impl in status_impl_strategy(),
            platforms in platforms_strategy(1..4),
        ) {
            let status = PlatformStatus::new(&status_impl);
            let empty = status.enabled_on(&PlatformSpec::Platforms(vec![]));
            let always = status.enabled_on(&PlatformSpec::Always);
            let some = status.enabled_on(&PlatformSpec::Platforms(platforms));
            let any = status.enabled_on(&PlatformSpec::Any);
            prop_assert!(empty <= always, "Platforms([]) <= Always: {:?} <= {:?}", empty, always);
            prop_assert!(always <= some, "Always <= Platforms(non-empty): {:?} <= {:?}", always, some);
            prop_assert!(some <= any, "Platforms(non-empty) <= Any: {:?} <= {:?}", some, any);
        }
    }
}
