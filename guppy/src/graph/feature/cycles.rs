// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Code for handling cycles in feature graphs.

use crate::{
    Error,
    graph::{
        FeatureIx,
        feature::{FeatureGraph, FeatureId},
    },
    petgraph_support::scc::Sccs,
};

/// Contains information about dependency cycles in feature graphs.
///
/// Cargo permits cycles if at least one of the links is dev-only. `Cycles` exposes information
/// about such dependencies.
///
/// Constructed through `PackageGraph::cycles`.
pub struct Cycles<'g> {
    feature_graph: FeatureGraph<'g>,
    sccs: &'g Sccs<FeatureIx>,
}

impl<'g> Cycles<'g> {
    pub(super) fn new(feature_graph: FeatureGraph<'g>) -> Self {
        Self {
            feature_graph,
            sccs: feature_graph.sccs(),
        }
    }

    /// Returns true if `a` and `b` lie on a common directed cycle in the
    /// feature graph.
    ///
    /// "Lie on a common cycle" means: in the same Strongly Connected
    /// Component *and* that SCC is non-trivial.
    ///
    /// * For distinct feature IDs, that's the same as being in a multi-node SCC.
    /// * For the same feature ID, the node must either be in a multi-node SCC,
    ///   or have a self-loop edge in the dependency graph (e.g., from a `path`
    ///   dev-dependency on the package's own crate).
    ///
    /// In particular, `is_cyclic(a, a)` is *not* reflexively true: it
    /// returns `false` for features that aren't on any cycle.
    pub fn is_cyclic<'a>(
        &self,
        a: impl Into<FeatureId<'a>>,
        b: impl Into<FeatureId<'a>>,
    ) -> Result<bool, Error> {
        let a = a.into();
        let b = b.into();
        let a_ix = self.feature_graph.feature_ix(a)?;
        let b_ix = self.feature_graph.feature_ix(b)?;

        if a_ix != b_ix {
            // Different features lie on a common cycle iff they're in the
            // same SCC -- which, for distinct features, can only be a
            // multi-node SCC.
            return Ok(self.sccs.is_same_scc(a_ix, b_ix));
        }

        // Same feature: on a cycle iff its SCC is non-trivial.
        Ok(
            self.sccs.in_multi_scc(a_ix)
                || self.feature_graph.dep_graph().contains_edge(a_ix, a_ix),
        )
    }

    /// Returns all the cyclic Strongly Connected Components of this graph:
    /// every multi-node SCC, plus every single-node SCC whose feature has
    /// a self-loop edge.
    ///
    /// Cycles are returned in topological order: if features in cycle B
    /// depend on features in cycle A, A is returned before B.
    ///
    /// Within a cycle, nodes are returned in non-dev order: if feature Foo
    /// has a dependency on Bar, and Bar has a dev-dependency on Foo, then
    /// Foo is returned before Bar.
    pub fn all_cycles(&self) -> impl Iterator<Item = Vec<FeatureId<'g>>> + 'g + use<'g> {
        let dep_graph = self.feature_graph.dep_graph();
        let package_graph = self.feature_graph.package_graph;
        self.sccs.all_sccs().filter_map(move |class| {
            let is_cyclic = match class {
                [_, _, ..] => true,
                &[ix] => dep_graph.contains_edge(ix, ix),
                [] => false,
            };
            is_cyclic.then(|| {
                class
                    .iter()
                    .map(move |feature_ix| {
                        FeatureId::from_node(package_graph, &dep_graph[*feature_ix])
                    })
                    .collect()
            })
        })
    }
}
