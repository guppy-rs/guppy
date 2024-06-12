// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use petgraph::{
    graph::IndexType,
    prelude::*,
    visit::{
        GraphRef, IntoNeighborsDirected, IntoNodeIdentifiers, NodeCompactIndexable, VisitMap,
        Visitable, Walker,
    },
};
use std::marker::PhantomData;

/// A cycle-aware topological sort of a graph.
#[derive(Clone, Debug)]
pub struct TopoWithCycles<Ix> {
    // This is a map of each node index to its corresponding topo index.
    reverse_index: Box<[usize]>,
    // Prevent mixing up index types.
    _phantom: PhantomData<Ix>,
}

impl<Ix: IndexType> TopoWithCycles<Ix> {
    pub fn new<G>(graph: G) -> Self
    where
        G: GraphRef
            + Visitable<NodeId = NodeIndex<Ix>>
            + IntoNodeIdentifiers
            + IntoNeighborsDirected<NodeId = NodeIndex<Ix>>
            + NodeCompactIndexable,
        G::Map: VisitMap<NodeIndex<Ix>>,
    {
        // petgraph's default topo algorithms don't handle cycles. Use DfsPostOrder which does.
        let mut dfs = DfsPostOrder::empty(graph);
        dfs.stack.extend(
            graph
                .node_identifiers()
                .filter(move |&a| graph.neighbors_directed(a, Incoming).next().is_none()),
        );
        let mut topo: Vec<NodeIndex<Ix>> = dfs.iter(graph).collect();
        // dfs returns its data in postorder (reverse topo order), so reverse that for forward topo
        // order.
        topo.reverse();

        // Because the graph is NodeCompactIndexable, the indexes are in the range
        // (0..graph.node_count()).
        // Use this property to build a reverse map.
        let mut reverse_index = vec![0; graph.node_count()];
        topo.iter().enumerate().for_each(|(topo_ix, node_ix)| {
            reverse_index[node_ix.index()] = topo_ix;
        });

        Self {
            reverse_index: reverse_index.into_boxed_slice(),
            _phantom: PhantomData,
        }
    }

    /// Sort nodes based on the topo order in self.
    #[inline]
    pub fn sort_nodes(&self, nodes: &mut [NodeIndex<Ix>]) {
        nodes.sort_unstable_by_key(|node_ix| self.topo_ix(*node_ix))
    }

    #[inline]
    pub fn topo_ix(&self, node_ix: NodeIndex<Ix>) -> usize {
        self.reverse_index[node_ix.index()]
    }
}
