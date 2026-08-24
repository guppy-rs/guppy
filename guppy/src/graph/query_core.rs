// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::{
    graph::{
        DependencyDirection, GraphSpec,
        ix_set::{IxOnes, IxSet},
    },
    petgraph_support::dfs::{BufferedEdgeFilter, dfs_next_buffered_filter},
};
use fixedbitset::FixedBitSet;
use petgraph::{
    graph::IndexType,
    prelude::*,
    visit::{IntoEdges, IntoNeighbors, Visitable},
};
use std::fmt;

pub(super) struct QueryParams<G: GraphSpec> {
    initials: IxSet<G>,
    direction: DependencyDirection,
}

impl<G: GraphSpec> QueryParams<G> {
    pub(super) fn new(
        graph: &Graph<G::Node, G::Edge, Directed, G::Ix>,
        initials: impl IntoIterator<Item = NodeIndex<G::Ix>>,
        direction: DependencyDirection,
    ) -> Self {
        Self {
            initials: IxSet::from_ixs(initials, graph.node_count()),
            direction,
        }
    }

    pub(super) fn direction(&self) -> DependencyDirection {
        self.direction
    }

    /// Returns true if this query specifies this node as an initial.
    pub(super) fn has_initial(&self, initial: NodeIndex<G::Ix>) -> bool {
        self.initials.contains(initial)
    }

    pub(super) fn initials(&self) -> IxOnes<'_, G> {
        self.initials.ones()
    }
}

// These manual impls avoid G: Clone/Debug bounds from derives.
impl<G: GraphSpec> Clone for QueryParams<G> {
    fn clone(&self) -> Self {
        Self {
            initials: self.initials.clone(),
            direction: self.direction,
        }
    }
}

impl<G: GraphSpec> fmt::Debug for QueryParams<G> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QueryParams")
            .field("initials", &self.initials)
            .field("direction", &self.direction)
            .finish()
    }
}

pub(super) fn all_visit_map<G, Ix>(graph: G) -> (FixedBitSet, usize)
where
    G: Visitable<NodeId = NodeIndex<Ix>, Map = FixedBitSet>,
    Ix: IndexType,
{
    let mut visit_map = graph.visit_map();
    // Mark all nodes visited.
    visit_map.insert_range(..);
    let len = visit_map.len();
    (visit_map, len)
}

pub(super) fn reachable_map<G, Ix>(
    graph: G,
    roots: impl IntoIterator<Item = G::NodeId>,
) -> (FixedBitSet, usize)
where
    G: Visitable<NodeId = NodeIndex<Ix>, Map = FixedBitSet> + IntoNeighbors,
    Ix: IndexType,
{
    // To figure out what nodes are reachable, run a DFS starting from the roots.
    // This is DfsPostOrder since that handles cycles while a regular DFS doesn't.
    let mut dfs = DfsPostOrder::empty(graph);
    dfs.stack = roots.into_iter().collect();
    while dfs.next(graph).is_some() {}

    // Once the DFS is done, the discovered map (or the finished map) is what's reachable.
    debug_assert_eq!(
        dfs.discovered, dfs.finished,
        "discovered and finished maps match at the end"
    );
    let reachable = dfs.discovered;
    let len = reachable.count_ones(..);
    (reachable, len)
}

pub(super) fn reachable_map_buffered_filter<G, Ix>(
    graph: G,
    mut filter: impl BufferedEdgeFilter<G>,
    roots: impl IntoIterator<Item = G::NodeId>,
) -> (FixedBitSet, usize)
where
    G: Visitable<NodeId = NodeIndex<Ix>, Map = FixedBitSet> + IntoEdges,
    Ix: IndexType,
{
    // To figure out what nodes are reachable, run a DFS starting from the roots.
    // This is DfsPostOrder since that handles cycles while a regular DFS doesn't.
    let mut dfs = DfsPostOrder::empty(graph);
    dfs.stack = roots.into_iter().collect();
    while dfs_next_buffered_filter(&mut dfs, graph, &mut filter).is_some() {}

    // Once the DFS is done, the discovered map (or the finished map) is what's reachable.
    debug_assert_eq!(
        dfs.discovered, dfs.finished,
        "discovered and finished maps match at the end"
    );
    let reachable = dfs.discovered;
    let len = reachable.count_ones(..);
    (reachable, len)
}
