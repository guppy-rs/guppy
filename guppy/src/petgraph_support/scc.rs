// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use ahash::AHashMap;
use fixedbitset::FixedBitSet;
use nested::Nested;
use petgraph::{
    algo::kosaraju_scc,
    graph::IndexType,
    prelude::*,
    visit::{IntoNeighborsDirected, IntoNodeIdentifiers, VisitMap, Visitable},
};
use std::slice;

#[derive(Clone, Debug)]
pub(crate) struct Sccs<Ix: IndexType> {
    sccs: Nested<Vec<NodeIndex<Ix>>>,
    // Map of node indexes to the index of the SCC they belong to. If a node is not part of an SCC,
    // then the corresponding index is not stored here.
    multi_map: AHashMap<NodeIndex<Ix>, usize>,
}

impl<Ix: IndexType> Sccs<Ix> {
    /// Creates a new instance from the provided graph and the given sorter.
    pub fn new<G>(graph: G, mut scc_sorter: impl FnMut(&mut Vec<NodeIndex<Ix>>)) -> Self
    where
        G: IntoNeighborsDirected<NodeId = NodeIndex<Ix>> + Visitable + IntoNodeIdentifiers,
        <G as Visitable>::Map: VisitMap<NodeIndex<Ix>>,
    {
        // Use kosaraju_scc since it is iterative (tarjan_scc is recursive) and package graphs
        // have unbounded depth.
        let sccs = kosaraju_scc(graph);
        let sccs: Nested<Vec<_>> = sccs
            .into_iter()
            .map(|mut scc| {
                if scc.len() > 1 {
                    scc_sorter(&mut scc);
                }
                scc
            })
            // kosaraju_scc returns its sccs in reverse topological order. Reverse it again for
            // forward topological order.
            .rev()
            .collect();
        let mut multi_map = AHashMap::new();
        for (idx, scc) in sccs.iter().enumerate() {
            if scc.len() > 1 {
                multi_map.extend(scc.iter().map(|ix| (*ix, idx)));
            }
        }
        Self { sccs, multi_map }
    }

    /// Returns true if `a` and `b` are in the same scc.
    pub fn is_same_scc(&self, a: NodeIndex<Ix>, b: NodeIndex<Ix>) -> bool {
        if a == b {
            return true;
        }
        match (self.multi_map.get(&a), self.multi_map.get(&b)) {
            (Some(a_scc), Some(b_scc)) => a_scc == b_scc,
            _ => false,
        }
    }

    /// Returns all the SCCs with more than one element.
    pub fn multi_sccs(&self) -> impl DoubleEndedIterator<Item = &[NodeIndex<Ix>]> {
        self.sccs.iter().filter(|scc| scc.len() > 1)
    }

    /// Returns all the nodes of this graph that have no incoming edges to them, and all the nodes
    /// in an SCC into which there are no incoming edges.
    pub fn externals<'a, G>(&'a self, graph: G) -> impl Iterator<Item = NodeIndex<Ix>> + 'a
    where
        G: 'a + IntoNodeIdentifiers + IntoNeighborsDirected<NodeId = NodeIndex<Ix>>,
        Ix: IndexType,
    {
        // Consider each SCC as one logical node.
        let mut external_sccs = FixedBitSet::with_capacity(self.sccs.len());
        let mut internal_sccs = FixedBitSet::with_capacity(self.sccs.len());
        graph
            .node_identifiers()
            .filter(move |ix| match self.multi_map.get(ix) {
                Some(&scc_idx) => {
                    // Consider one node identifier for each scc -- whichever one comes first.
                    if external_sccs.contains(scc_idx) {
                        return true;
                    }
                    if internal_sccs.contains(scc_idx) {
                        return false;
                    }

                    let scc = &self.sccs[scc_idx];
                    let is_external = scc
                        .iter()
                        .flat_map(|ix| {
                            // Look at all incoming nodes from every SCC member.
                            graph.neighbors_directed(*ix, Incoming)
                        })
                        .all(|neighbor_ix| {
                            // * Accept any nodes are in the same SCC.
                            // * Any other results imply that this isn't an external scc.
                            match self.multi_map.get(&neighbor_ix) {
                                Some(neighbor_scc_idx) => neighbor_scc_idx == &scc_idx,
                                None => false,
                            }
                        });
                    if is_external {
                        external_sccs.insert(scc_idx);
                    } else {
                        internal_sccs.insert(scc_idx);
                    }
                    is_external
                }
                None => {
                    // Not part of a multi-node SCC. Treat the node as its own
                    // (single-element) SCC: it's external iff it has no
                    // incoming neighbors *other than itself*.
                    //
                    // A self-loop is an edge within the node's own SCC and must
                    // not disqualify it from being external, matching how the
                    // multi-node branch above accepts in-SCC neighbors.
                    !graph
                        .neighbors_directed(*ix, Incoming)
                        .any(|neighbor_ix| neighbor_ix != *ix)
                }
            })
    }

    /// Iterate over all nodes in the direction specified.
    pub fn node_iter(&self, direction: Direction) -> NodeIter<'_, Ix> {
        NodeIter {
            node_ixs: self.sccs.data().iter(),
            direction,
        }
    }
}

/// An iterator over the nodes of strongly connected components.
#[derive(Clone, Debug)]
pub(crate) struct NodeIter<'a, Ix> {
    node_ixs: slice::Iter<'a, NodeIndex<Ix>>,
    direction: Direction,
}

impl<Ix> NodeIter<'_, Ix> {
    /// Returns the direction this iteration is happening in.
    #[allow(dead_code)]
    pub fn direction(&self) -> Direction {
        self.direction
    }
}

impl<Ix: IndexType> Iterator for NodeIter<'_, Ix> {
    type Item = NodeIndex<Ix>;

    fn next(&mut self) -> Option<NodeIndex<Ix>> {
        // Note that outgoing implies iterating over the sccs in forward order, while incoming means
        // sccs in reverse order.
        match self.direction {
            Direction::Outgoing => self.node_ixs.next().copied(),
            Direction::Incoming => self.node_ixs.next_back().copied(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use petgraph::Graph;
    use std::collections::HashSet;

    /// Self-loops are internal to a node's own (single-element) SCC, so
    /// they neither qualify a node for nor disqualify it from being a
    /// forward root on their own.
    #[test]
    fn externals_with_self_loops_on_single_node_sccs() {
        // a -> a (self-loop, no external incoming): `a` is external.
        // a -> b (real incoming edge for `b`)
        // b -> b (self-loop; the real incoming wins): `b` is not external.
        let mut graph = Graph::<(), (), Directed, u32>::new();
        let a = graph.add_node(());
        let b = graph.add_node(());
        graph.add_edge(a, a, ());
        graph.add_edge(a, b, ());
        graph.add_edge(b, b, ());

        let sccs = Sccs::<u32>::new(&graph, |_| {});
        let externals: HashSet<NodeIndex<u32>> = sccs.externals(&graph).collect();
        assert_eq!(externals, HashSet::from([a]));
    }

    /// A self-loop on a node that belongs to a multi-node SCC must not
    /// disqualify the SCC from being external.
    #[test]
    fn externals_with_self_loop_inside_multi_node_scc() {
        // a -> b, b -> a (mutual edges form a 2-node SCC {a, b})
        // a -> a (self-loop, still inside the SCC)
        // a -> c
        //
        // * The SCC {a, b} has no incoming edge from outside, so both of
        //   its members are externals.
        // * `c` is reachable from the SCC and is not external.
        let mut graph = Graph::<(), (), Directed, u32>::new();
        let a = graph.add_node(());
        let b = graph.add_node(());
        let c = graph.add_node(());
        graph.add_edge(a, b, ());
        graph.add_edge(b, a, ());
        graph.add_edge(a, a, ());
        graph.add_edge(a, c, ());

        let sccs = Sccs::<u32>::new(&graph, |_| {});
        let externals: HashSet<NodeIndex<u32>> = sccs.externals(&graph).collect();
        assert_eq!(externals, HashSet::from([a, b]));
    }
}
