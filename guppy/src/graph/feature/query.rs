// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::{
    Error, PackageId,
    graph::{
        DependencyDirection, FeatureIx, PackageMetadata,
        feature::{ConditionalLink, FeatureGraph, FeatureId, FeatureLabel, FeatureSet},
    },
};
use itertools::Itertools;
use petgraph::graph::NodeIndex;
use std::collections::HashSet;

/// Trait representing whether a feature within a package should be selected.
///
/// This is conceptually similar to passing `--features` or other similar command-line options to
/// Cargo.
///
/// Most uses will involve using one of the predefined filters: `all_filter`, `default_filter`, or
/// `none_filter`. A customized filter can be provided either through `filter_fn` or by implementing
/// this trait.
pub trait FeatureFilter<'g> {
    /// Returns true if this feature ID should be selected in the graph.
    ///
    /// Returning false does not prevent this feature ID from being included if it's reachable
    /// through other means.
    ///
    /// In general, `accept` should return true if `feature_id.is_base()` is true.
    ///
    /// The feature ID is guaranteed to be in this graph, so it is OK to panic if it isn't found.
    fn accept(&mut self, graph: &FeatureGraph<'g>, feature_id: FeatureId<'g>) -> bool;
}

impl<'g, T> FeatureFilter<'g> for &mut T
where
    T: FeatureFilter<'g>,
{
    fn accept(&mut self, graph: &FeatureGraph<'g>, feature_id: FeatureId<'g>) -> bool {
        (**self).accept(graph, feature_id)
    }
}

impl<'g> FeatureFilter<'g> for Box<dyn FeatureFilter<'g> + '_> {
    fn accept(&mut self, graph: &FeatureGraph<'g>, feature_id: FeatureId<'g>) -> bool {
        (**self).accept(graph, feature_id)
    }
}

impl<'g> FeatureFilter<'g> for &mut dyn FeatureFilter<'g> {
    fn accept(&mut self, graph: &FeatureGraph<'g>, feature_id: FeatureId<'g>) -> bool {
        (**self).accept(graph, feature_id)
    }
}

/// A `FeatureFilter` which calls the function that's passed in.
#[derive(Clone, Debug)]
pub struct FeatureFilterFn<F>(F);

impl<'g, F> FeatureFilterFn<F>
where
    F: FnMut(&FeatureGraph<'g>, FeatureId<'g>) -> bool,
{
    /// Returns a new instance of this wrapper.
    pub fn new(f: F) -> Self {
        FeatureFilterFn(f)
    }
}

impl<'g, F> FeatureFilter<'g> for FeatureFilterFn<F>
where
    F: FnMut(&FeatureGraph<'g>, FeatureId<'g>) -> bool,
{
    fn accept(&mut self, graph: &FeatureGraph<'g>, feature_id: FeatureId<'g>) -> bool {
        (self.0)(graph, feature_id)
    }
}

/// Describes one of the standard sets of features recognized by Cargo: none, all or default.
///
/// `StandardFeatures` implements `FeatureFilter<'g>`, so it can be passed in as a feature filter
/// wherever necessary.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialOrd, PartialEq)]
pub enum StandardFeatures {
    /// No features. Equivalent to a build with `--no-default-features`.
    None,

    /// Default features. Equivalent to a standard `cargo build`.
    Default,

    /// All features. Equivalent to `cargo build --all-features`.
    All,
}

impl StandardFeatures {
    /// A list of all the possible values of `StandardFeatures`.
    pub const VALUES: &'static [Self; 3] = &[
        StandardFeatures::None,
        StandardFeatures::Default,
        StandardFeatures::All,
    ];
}

impl<'g> FeatureFilter<'g> for StandardFeatures {
    fn accept(&mut self, graph: &FeatureGraph<'g>, feature_id: FeatureId<'g>) -> bool {
        match self {
            StandardFeatures::None => {
                // The only feature ID that should be accepted is the base one.
                feature_id.is_base()
            }
            StandardFeatures::Default => {
                // XXX it kinda sucks that we already know about the exact feature ixs but need to go
                // through the feature ID over here. Might be worth reorganizing the code to not do that.
                graph
                    .is_default_feature(feature_id)
                    .expect("feature IDs should be valid")
            }
            StandardFeatures::All => true,
        }
    }
}

/// Returns a `FeatureFilter` that selects everything from the base filter, plus these additional
/// feature names -- regardless of what package they are in.
///
/// This is equivalent to a build with `--features`, and is typically meant to be used with one
/// package.
///
/// For filtering by feature IDs, use `feature_id_filter`.
pub fn named_feature_filter<'g: 'a, 'a>(
    base: impl FeatureFilter<'g> + 'a,
    features: impl IntoIterator<Item = &'a str>,
) -> impl FeatureFilter<'g> + 'a {
    let mut base = base;
    let features: HashSet<_> = features.into_iter().collect();
    FeatureFilterFn::new(move |feature_graph, feature_id| {
        if base.accept(feature_graph, feature_id) {
            return true;
        }
        match feature_id.label() {
            FeatureLabel::Named(feature) => features.contains(feature),
            _ => {
                // This is the base feature. Assume that it has already been selected by the base
                // filter.
                false
            }
        }
    })
}

/// Returns a `FeatureFilter` that selects everything from the base filter, plus some additional
/// feature IDs.
///
/// This is a more advanced version of `feature_filter`.
pub fn feature_id_filter<'g: 'a, 'a>(
    base: impl FeatureFilter<'g> + 'a,
    feature_ids: impl IntoIterator<Item = impl Into<FeatureId<'a>>>,
) -> impl FeatureFilter<'g> + 'a {
    let mut base = base;
    let feature_ids: HashSet<_> = feature_ids
        .into_iter()
        .map(|feature_id| feature_id.into())
        .collect();
    FeatureFilterFn::new(move |feature_graph, feature_id| {
        base.accept(feature_graph, feature_id) || feature_ids.contains(&feature_id)
    })
}

/// A query over a feature graph.
///
/// A `FeatureQuery` is the entry point for Cargo resolution, and also provides iterators over
/// feature IDs and links. This struct is constructed through the `query_` methods on
/// `FeatureGraph`, or through `PackageQuery::to_feature_query`.
#[derive(Clone, Debug)]
pub struct FeatureQuery<'g> {
    pub(super) initials: FeatureSet<'g>,
    pub(super) direction: DependencyDirection,
}

assert_covariant!(FeatureQuery);

/// ## Queries
///
/// The methods in this section create queries over subsets of this feature graph. Use the methods
/// here to analyze transitive dependencies.
impl<'g> FeatureGraph<'g> {
    /// Creates a new query over the entire workspace.
    ///
    /// `query_workspace` will select all workspace packages (subject to the provided filter) and
    /// their transitive dependencies.
    pub fn query_workspace(&self, filter: impl FeatureFilter<'g>) -> FeatureQuery<'g> {
        self.package_graph
            .query_workspace()
            .to_feature_query(filter)
    }

    /// Creates a new query that returns transitive dependencies of the given feature IDs in the
    /// specified direction.
    ///
    /// Returns an error if any feature IDs are unknown.
    pub fn query_directed<'a>(
        &self,
        feature_ids: impl IntoIterator<Item = impl Into<FeatureId<'a>>>,
        dep_direction: DependencyDirection,
    ) -> Result<FeatureQuery<'g>, Error> {
        match dep_direction {
            DependencyDirection::Forward => self.query_forward(feature_ids),
            DependencyDirection::Reverse => self.query_reverse(feature_ids),
        }
    }

    /// Creates a new query that returns transitive dependencies of the given feature IDs.
    ///
    /// Returns an error if any feature IDs are unknown.
    pub fn query_forward<'a>(
        &self,
        feature_ids: impl IntoIterator<Item = impl Into<FeatureId<'a>>>,
    ) -> Result<FeatureQuery<'g>, Error> {
        let feature_ids = feature_ids.into_iter().map(|feature_id| feature_id.into());
        let feature_ixs: Vec<_> = self.feature_ixs(feature_ids)?;
        Ok(self.query_from_parts(feature_ixs, DependencyDirection::Forward))
    }

    /// Creates a new query that returns transitive reverse dependencies of the given feature IDs.
    ///
    /// Returns an error if any feature IDs are unknown.
    pub fn query_reverse<'a>(
        &self,
        feature_ids: impl IntoIterator<Item = impl Into<FeatureId<'a>>>,
    ) -> Result<FeatureQuery<'g>, Error> {
        let feature_ids = feature_ids.into_iter().map(|feature_id| feature_id.into());
        let feature_ixs: Vec<_> = self.feature_ixs(feature_ids)?;
        Ok(self.query_from_parts(feature_ixs, DependencyDirection::Reverse))
    }

    pub(in crate::graph) fn query_from_parts(
        &self,
        feature_ixs: impl IntoIterator<Item = NodeIndex<FeatureIx>>,
        direction: DependencyDirection,
    ) -> FeatureQuery<'g> {
        FeatureQuery {
            initials: FeatureSet::from_ixs(*self, feature_ixs),
            direction,
        }
    }
}

impl<'g> FeatureQuery<'g> {
    /// Returns the feature graph the query is going to be executed on.
    pub fn graph(&self) -> &FeatureGraph<'g> {
        self.initials.graph()
    }

    /// Returns the direction the query is happening in.
    pub fn direction(&self) -> DependencyDirection {
        self.direction
    }

    /// Returns the set of initial features specified in the query.
    pub fn initials(&self) -> &FeatureSet<'g> {
        &self.initials
    }

    /// Returns the list of initial packages specified in the query.
    ///
    /// The order of packages is unspecified.
    pub fn initial_packages<'a>(&'a self) -> impl Iterator<Item = PackageMetadata<'g>> + 'a {
        let graph = *self.graph();
        // sorted_ixs() iterates in ascending ix order and each package's
        // feature ixs are contiguous, so dedup() is fine.
        self.initials
            .sorted_ixs()
            .map(move |feature_ix| graph.metadata_for_ix(feature_ix).package())
            .dedup()
    }

    /// Returns true if the query starts from the given package.
    ///
    /// Returns an error if the package ID is unknown.
    pub fn starts_from_package(&self, package_id: &PackageId) -> Result<bool, Error> {
        self.initials.contains_package(package_id)
    }

    /// Resolves this query into a set of known feature IDs.
    ///
    /// This is the entry point for iterators.
    pub fn resolve(self) -> FeatureSet<'g> {
        FeatureSet::new(self)
    }

    /// Resolves this query into a set of known feature IDs, using the provided visitor to
    /// determine which links are followed.
    pub fn resolve_with(self, visitor: impl FeatureLinkVisitor<'g>) -> FeatureSet<'g> {
        FeatureSet::with_link_visitor(self, visitor)
    }

    /// Resolves this query into a set of known feature IDs, using the provided visitor function to
    /// determine which links are followed.
    pub fn resolve_with_fn(
        self,
        visitor_fn: impl FnMut(&FeatureLinkContext<'g>, ConditionalLink<'g>) -> bool,
    ) -> FeatureSet<'g> {
        self.resolve_with(LinkVisitorFn(visitor_fn))
    }
}

/// Context passed to a [`FeatureLinkVisitor`] for each link visited during a
/// resolve operation.
#[derive(Clone, Debug)]
pub struct FeatureLinkContext<'g> {
    query: FeatureQuery<'g>,
}

impl<'g> FeatureLinkContext<'g> {
    pub(super) fn new(query: FeatureQuery<'g>) -> Self {
        Self { query }
    }

    /// Returns the query this resolve operation was started from.
    pub fn query(&self) -> &FeatureQuery<'g> {
        &self.query
    }

    /// Returns the direction of the traversal.
    pub fn direction(&self) -> DependencyDirection {
        self.query.direction()
    }

    /// Returns true if the link's starting endpoint (`from` for forward
    /// queries, `to` for reverse queries) is one of the query's initials.
    pub fn starts_from_initial(&self, link: &ConditionalLink<'g>) -> bool {
        let feature_ix = match self.direction() {
            DependencyDirection::Forward => link.from().feature_ix(),
            DependencyDirection::Reverse => link.to().feature_ix(),
        };
        self.query.initials.contains_ix(feature_ix)
    }
}

/// Represents whether a particular link within a feature graph should be followed during a
/// resolve operation.
pub trait FeatureLinkVisitor<'g> {
    /// Returns true if this conditional link should be followed during a resolve operation.
    fn visit_link(&mut self, cx: &FeatureLinkContext<'g>, link: ConditionalLink<'g>) -> bool;
}

impl<'g, T> FeatureLinkVisitor<'g> for &mut T
where
    T: FeatureLinkVisitor<'g>,
{
    fn visit_link(&mut self, cx: &FeatureLinkContext<'g>, link: ConditionalLink<'g>) -> bool {
        (**self).visit_link(cx, link)
    }
}

impl<'g> FeatureLinkVisitor<'g> for Box<dyn FeatureLinkVisitor<'g> + '_> {
    fn visit_link(&mut self, cx: &FeatureLinkContext<'g>, link: ConditionalLink<'g>) -> bool {
        (**self).visit_link(cx, link)
    }
}

impl<'g> FeatureLinkVisitor<'g> for &mut dyn FeatureLinkVisitor<'g> {
    fn visit_link(&mut self, cx: &FeatureLinkContext<'g>, link: ConditionalLink<'g>) -> bool {
        (**self).visit_link(cx, link)
    }
}

#[derive(Clone, Debug)]
struct LinkVisitorFn<F>(pub F);

impl<'g, F> FeatureLinkVisitor<'g> for LinkVisitorFn<F>
where
    F: FnMut(&FeatureLinkContext<'g>, ConditionalLink<'g>) -> bool,
{
    fn visit_link(&mut self, cx: &FeatureLinkContext<'g>, link: ConditionalLink<'g>) -> bool {
        (self.0)(cx, link)
    }
}
