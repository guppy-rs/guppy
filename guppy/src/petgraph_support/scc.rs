// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use fixedbitset::FixedBitSet;
use petgraph::{
    algo::kosaraju_scc,
    graph::IndexType,
    prelude::*,
    visit::{IntoNeighborsDirected, IntoNodeIdentifiers, NodeIndexable, VisitMap, Visitable},
};
use std::slice;

#[derive(Clone, Debug)]
pub(crate) struct Sccs<Ix: IndexType> {
    /// All nodes in topological order. Each SCC is a contiguous subsequence,
    /// stored in the order returned by the SCC sorter.
    order: Vec<NodeIndex<Ix>>,
    /// Each SCC's start in `order`, plus a sentinel at the end with the value
    /// `order.len()`.
    scc_bounds: Vec<Ix>,
    /// The index of the SCC each node is in, by node index.
    scc_of: Vec<Ix>,
}

impl<Ix: IndexType> Sccs<Ix> {
    /// Creates a new instance from the provided graph and the given sorter.
    pub fn new<G>(graph: G, mut scc_sorter: impl FnMut(&mut Vec<NodeIndex<Ix>>)) -> Self
    where
        G: IntoNeighborsDirected<NodeId = NodeIndex<Ix>>
            + Visitable
            + IntoNodeIdentifiers
            + NodeIndexable,
        <G as Visitable>::Map: VisitMap<NodeIndex<Ix>>,
    {
        let node_bound = graph.node_bound();
        // Use kosaraju_scc since it is iterative (tarjan_scc is recursive) and
        // package graphs have unbounded depth.
        let sccs = kosaraju_scc(graph);
        let mut order = Vec::with_capacity(sccs.iter().map(Vec::len).sum());
        let mut scc_bounds = Vec::with_capacity(sccs.len() + 1);
        // kosaraju_scc returns its sccs in reverse topological order. Reverse
        // it again for forward topological order.
        for mut scc in sccs.into_iter().rev() {
            if scc.len() > 1 {
                scc_sorter(&mut scc);
            }
            scc_bounds.push(Ix::new(order.len()));
            order.extend(scc);
        }
        scc_bounds.push(Ix::new(order.len()));

        let mut scc_of = vec![Ix::new(0); node_bound];
        for (scc_idx, bounds) in scc_bounds.windows(2).enumerate() {
            let (start, end) = (bounds[0].index(), bounds[1].index());
            for ix in &order[start..end] {
                scc_of[ix.index()] = Ix::new(scc_idx);
            }
        }

        Self {
            order,
            scc_bounds,
            scc_of,
        }
    }

    /// Returns true if `a` and `b` are in the same scc.
    ///
    /// Every node is in its own (possibly single-element) SCC, so this is
    /// reflexive: `is_same_scc(a, a)` is always `true`. This is the SCC
    /// equivalence relation; it is *not* a cycle-membership predicate.
    /// Callers that want to know whether two nodes lie on a common cycle
    /// should additionally check [`Sccs::in_multi_scc`] and/or whether the
    /// node has a self-loop edge.
    pub fn is_same_scc(&self, a: NodeIndex<Ix>, b: NodeIndex<Ix>) -> bool {
        a == b || self.scc_of[a.index()] == self.scc_of[b.index()]
    }

    /// Returns true if `ix` belongs to an SCC with more than one element.
    ///
    /// A multi-node SCC is, by definition, non-trivial: every pair of its
    /// members lies on a directed cycle. Combined with a self-loop check on
    /// `ix`, this is enough to decide whether a node lies on any cycle.
    pub fn in_multi_scc(&self, ix: NodeIndex<Ix>) -> bool {
        self.multi_scc(ix).is_some()
    }

    /// Returns all the SCCs of this graph in forward topological order,
    /// including single-node SCCs.
    ///
    /// [`Sccs`] does not store edge information, so callers that care
    /// about *cyclic* SCCs -- multi-node SCCs plus single-node SCCs with a
    /// self-loop -- must consult the underlying graph for the self-loop
    /// check themselves.
    pub fn all_sccs(&self) -> impl DoubleEndedIterator<Item = &[NodeIndex<Ix>]> {
        self.scc_bounds
            .windows(2)
            .map(|bounds| &self.order[bounds[0].index()..bounds[1].index()])
    }

    /// Returns all the nodes that have no incoming edges from outside their
    /// own SCC.
    ///
    /// Edges *within* an SCC -- including self-loop edges on single-node SCCs
    /// -- don't disqualify a node. The result is one representative per
    /// external multi-node SCC, plus every single-node SCC whose only
    /// incoming edge (if any) is a self-loop.
    pub fn externals<'a, G>(&'a self, graph: G) -> impl Iterator<Item = NodeIndex<Ix>> + 'a
    where
        G: 'a + IntoNodeIdentifiers + IntoNeighborsDirected<NodeId = NodeIndex<Ix>>,
        Ix: IndexType,
    {
        // Consider each SCC as one logical node.
        let mut external_sccs = FixedBitSet::with_capacity(self.scc_count());
        let mut internal_sccs = FixedBitSet::with_capacity(self.scc_count());
        graph
            .node_identifiers()
            .filter(move |ix| match self.multi_scc(*ix) {
                Some(scc_idx) => {
                    // Consider one node identifier for each scc -- whichever one comes first.
                    if external_sccs.contains(scc_idx) {
                        return true;
                    }
                    if internal_sccs.contains(scc_idx) {
                        return false;
                    }

                    let is_external = self
                        .scc_members(scc_idx)
                        .iter()
                        .flat_map(|ix| {
                            // Look at all incoming nodes from every SCC member.
                            graph.neighbors_directed(*ix, Incoming)
                        })
                        .all(|neighbor_ix| {
                            // * Accept any nodes are in the same SCC.
                            // * Any other results imply that this isn't an external scc.
                            self.scc_index(neighbor_ix) == scc_idx
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
            node_ixs: self.order.iter(),
            direction,
        }
    }

    fn scc_count(&self) -> usize {
        self.scc_bounds.len() - 1
    }

    fn scc_members(&self, scc_idx: usize) -> &[NodeIndex<Ix>] {
        &self.order[self.scc_start(scc_idx)..self.scc_start(scc_idx + 1)]
    }

    fn scc_start(&self, scc_idx: usize) -> usize {
        self.scc_bounds[scc_idx].index()
    }

    fn scc_index(&self, ix: NodeIndex<Ix>) -> usize {
        self.scc_of[ix.index()].index()
    }

    fn multi_scc(&self, ix: NodeIndex<Ix>) -> Option<usize> {
        let scc_idx = self.scc_index(ix);
        let scc_len = self.scc_start(scc_idx + 1) - self.scc_start(scc_idx);
        (scc_len > 1).then_some(scc_idx)
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
