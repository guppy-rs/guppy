// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::petgraph_support::NodeSet;
use ahash::AHashMap;
use fixedbitset::FixedBitSet;
use petgraph::{
    algo::kosaraju_scc,
    graph::IndexType,
    prelude::*,
    visit::{
        FilterNode, IntoNeighborsDirected, IntoNodeIdentifiers, NodeIndexable, VisitMap, Visitable,
    },
};
use std::{slice, vec};

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
    // The indexes of multi-node SCCs, so that `for_subset`'s fast path doesn't
    // scan every single-node SCC.
    multi_sccs: Vec<usize>,
    // Map of each node's position in `order`, indexed by node index.
    // Sorting a set's members by this list provides a topological order.
    positions: Vec<Ix>,
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
        let mut multi_sccs = Vec::new();
        for (scc_idx, bounds) in scc_bounds.windows(2).enumerate() {
            let (start, end) = (bounds[0].index(), bounds[1].index());
            for ix in &order[start..end] {
                scc_of[ix.index()] = Ix::new(scc_idx);
            }
            if end - start > 1 {
                multi_sccs.push(scc_idx);
            }
        }
        let mut positions = vec![Ix::new(0); node_bound];
        for (position, ix) in order.iter().enumerate() {
            positions[ix.index()] = Ix::new(position);
        }

        Self {
            order,
            scc_bounds,
            scc_of,
            multi_sccs,
            positions,
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

    /// Returns SCCs of the subgraph induced by `included`.
    ///
    /// This refines the full graph's SCCs, since removing a node can cause a
    /// single SCC to be split into two or more SCCs.
    pub fn for_subset<N, E>(
        &self,
        graph: &Graph<N, E, Directed, Ix>,
        included: impl FilterNode<NodeIndex<Ix>>,
    ) -> SubsetSccs<'_, Ix> {
        let mut split: Option<Box<SplitSccs<Ix>>> = None;
        for &scc_idx in &self.multi_sccs {
            let scc = self.scc_members(scc_idx);
            let included_count = scc.iter().filter(|ix| included.include_node(**ix)).count();
            if included_count == 0 || included_count == scc.len() {
                continue;
            }

            let members: Vec<_> = scc
                .iter()
                .copied()
                .filter(|ix| included.include_node(*ix))
                .collect();
            let split = split.get_or_insert_with(Default::default);

            // Renumber nodes within this SCC to account for split SCCs.
            //
            // Let's say the full topo order is:
            //
            // position:  0   1   2   3   4   5
            // node:      a   b   c   d   e   f
            //               └SCC {b,c,d,e}┘
            //
            // The SCC {b, c, d, e} occupies positions 1-4. Let's suppose now
            // that the subset excludes c, and let's suppose this breaks the
            // cycle into `{e} -> {b, d}` in topological order. Then, the
            // members get renumbered like this:
            //
            // * a -> 0 (not part of the SCC)
            // * e -> 1
            // * b -> 2
            // * d -> 3
            // * (4 is unused)
            // * f -> 5 (not part of the SCC)
            //
            // In this example:
            //
            // * The order inside the original SCC changes.
            // * The order relative to all other nodes stays the same.
            //
            // The gap (unused position 4) is harmless because the new positions
            // are only used as sort keys, and because node_iter does not follow
            // the NodeIter::Walk path if there are any split SCCs.
            let start = self.scc_start(scc_idx);
            let mut position = start;
            let mut nodes = vec![None; scc.len()];
            for sub_scc in split_scc(graph, &members) {
                let multi_scc = if sub_scc.len() > 1 {
                    Some(self.scc_count() + split.sub_sccs.len())
                } else {
                    None
                };
                for &ix in &sub_scc {
                    nodes[self.position(ix) - start] = Some(SplitNode {
                        position,
                        multi_scc,
                    });
                    position += 1;
                }
                if multi_scc.is_some() {
                    split.sub_sccs.push(sub_scc);
                }
            }
            split.sccs.push(SplitScc { scc_idx, nodes });
        }
        SubsetSccs { sccs: self, split }
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

    fn position(&self, ix: NodeIndex<Ix>) -> usize {
        self.positions[ix.index()].index()
    }

    fn multi_scc(&self, ix: NodeIndex<Ix>) -> Option<usize> {
        let scc_idx = self.scc_index(ix);
        let scc_len = self.scc_start(scc_idx + 1) - self.scc_start(scc_idx);
        (scc_len > 1).then_some(scc_idx)
    }
}

/// SCCs of an induced subgraph.
///
/// Returned by `Sccs::for_subset`.
///
/// Set-level queries live here rather than on `Sccs`, so that they can't
/// be answered with the whole graph's SCCs by mistake. In the common case
/// with no split SCCs, `split` is `None`.
#[derive(Clone, Debug)]
pub(crate) struct SubsetSccs<'a, Ix: IndexType> {
    sccs: &'a Sccs<Ix>,
    split: Option<Box<SplitSccs<Ix>>>,
}

#[derive(Clone, Debug)]
struct SplitSccs<Ix: IndexType> {
    // Split SCCs, in increasing SCC index order.
    sccs: Vec<SplitScc>,
    // Multi-node SCCs created by splitting.
    //
    // Multi-node SCC IDs, as returned by `SubsetSccs::multi_scc`, share a
    // single number space:
    //
    // * IDs below `Sccs::scc_count()` are SCCs of the whole graph that the
    //   subset either includes in full or excludes entirely.
    // * An ID at or past `Sccs::scc_count()` refers to
    //   `sub_sccs[id - scc_count]`.
    //
    // For example, if the whole graph has 6 SCCs and the subset splits
    // {b, c, d, e} into {e} and {b, d}, then {b, d} gets ID 6 and is stored
    // at `sub_sccs[0]`. {e} is a single node, so it gets no ID and isn't
    // stored here.
    //
    // A split SCC's original ID is never reported for any of its members, so
    // the two ranges can't alias.
    //
    // The number space is shared so that `externals` can key a single pair of
    // bitsets by SCC ID. (We should profile a few different options here,
    // including an enum or similar, to figure out if there's a better
    // representation.)
    sub_sccs: Vec<Vec<NodeIndex<Ix>>>,
}

impl<Ix: IndexType> Default for SplitSccs<Ix> {
    fn default() -> Self {
        Self {
            sccs: Vec::new(),
            sub_sccs: Vec::new(),
        }
    }
}

impl<Ix: IndexType> SplitSccs<Ix> {
    fn node(&self, sccs: &Sccs<Ix>, ix: NodeIndex<Ix>) -> Option<SplitNode> {
        let scc_idx = sccs.scc_index(ix);
        let split_idx = self
            .sccs
            .binary_search_by_key(&scc_idx, |split_scc| split_scc.scc_idx)
            .ok()?;
        let offset = sccs.position(ix) - sccs.scc_start(scc_idx);
        self.sccs[split_idx].nodes[offset]
    }
}

#[derive(Clone, Debug)]
struct SplitScc {
    scc_idx: usize,
    // Indexed by offset in the SCC's run. `None` if excluded.
    nodes: Vec<Option<SplitNode>>,
}

#[derive(Clone, Copy, Debug)]
struct SplitNode {
    position: usize,
    multi_scc: Option<usize>,
}

/// Sets with fewer members than (graph node count / this value) are ordered by
/// sorting their members instead of walking every node in the graph.
///
/// This is a relatively coarse threshold derived from some quick benchmarking —
/// we may want to tune it in the future.
const SORT_THRESHOLD: usize = 16;

impl<'a, Ix: IndexType> SubsetSccs<'a, Ix> {
    /// Returns all the nodes that have no incoming edges from outside their
    /// own SCC.
    ///
    /// `graph` is the (possibly reversed) graph filtered to `included`.
    ///
    /// Edges *within* an SCC -- including self-loop edges on single-node SCCs
    /// -- don't disqualify a node. The result is every member of each
    /// external multi-node SCC, plus every single-node SCC whose only
    /// incoming edge (if any) is a self-loop.
    ///
    /// Results are produced in `node_iter(direction)` order, so that roots
    /// follow topological order and a cycle's members stay together.
    pub fn externals<'b, G, S>(
        &'b self,
        graph: G,
        included: S,
        direction: Direction,
    ) -> impl Iterator<Item = NodeIndex<Ix>> + 'b
    where
        G: 'b + IntoNeighborsDirected<NodeId = NodeIndex<Ix>>,
        S: 'b + NodeSet<Ix>,
    {
        let scc_count =
            self.sccs.scc_count() + self.split.as_ref().map_or(0, |split| split.sub_sccs.len());
        // Consider each SCC as one logical node.
        let mut external_sccs = FixedBitSet::with_capacity(scc_count);
        let mut internal_sccs = FixedBitSet::with_capacity(scc_count);
        self.node_iter(included, direction)
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
                        .multi_scc_members(scc_idx)
                        .iter()
                        .flat_map(|ix| {
                            // Look at all incoming nodes from every SCC member.
                            graph.neighbors_directed(*ix, Incoming)
                        })
                        // * Accept any nodes are in the same SCC.
                        // * Any other results imply that this isn't an external scc.
                        .all(|neighbor_ix| self.multi_scc(neighbor_ix) == Some(scc_idx));
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

    /// Iterates over the set's members in topological order.
    pub fn node_iter<S: NodeSet<Ix>>(
        &self,
        included: S,
        direction: Direction,
    ) -> NodeIter<'a, Ix, S> {
        let node_count = self.sccs.positions.len();
        // Important! The walk iterator is only correct if self.split is None.
        // That is because it iterates `self.sccs.order` directly and ignores
        // renumbered positions caused by split SCCs.
        if self.split.is_none()
            && included.member_count().saturating_mul(SORT_THRESHOLD) >= node_count
        {
            return NodeIter::Walk {
                iter: self.sccs.order.iter(),
                included,
                direction,
            };
        }

        let mut sorted: Vec<_> = included
            .members()
            .map(|ix| (self.position(ix), ix))
            .collect();
        sorted.sort_unstable_by_key(|(position, _)| *position);
        NodeIter::Sorted {
            iter: sorted.into_iter(),
            direction,
        }
    }

    fn position(&self, ix: NodeIndex<Ix>) -> usize {
        match self.split_node(ix) {
            Some(split_node) => split_node.position,
            None => self.sccs.position(ix),
        }
    }

    fn multi_scc(&self, ix: NodeIndex<Ix>) -> Option<usize> {
        match self.split_node(ix) {
            Some(split_node) => split_node.multi_scc,
            None => self.sccs.multi_scc(ix),
        }
    }

    fn multi_scc_members(&self, scc_idx: usize) -> &[NodeIndex<Ix>] {
        let global_count = self.sccs.scc_count();
        if scc_idx < global_count {
            self.sccs.scc_members(scc_idx)
        } else {
            let split = self
                .split
                .as_ref()
                .expect("SCC IDs past the global count come from a split");
            &split.sub_sccs[scc_idx - global_count]
        }
    }

    // Kept small so the common unsplit case inlines well.
    fn split_node(&self, ix: NodeIndex<Ix>) -> Option<SplitNode> {
        match &self.split {
            Some(split) => split.node(self.sccs, ix),
            None => None,
        }
    }

    #[cfg(test)]
    fn is_split(&self) -> bool {
        self.split.is_some()
    }

    #[cfg(test)]
    fn is_same_scc(&self, a: NodeIndex<Ix>, b: NodeIndex<Ix>) -> bool {
        if a == b {
            return true;
        }
        match (self.multi_scc(a), self.multi_scc(b)) {
            (Some(a_scc), Some(b_scc)) => a_scc == b_scc,
            _ => false,
        }
    }
}

/// Splits `members`, part of one SCC, into the SCCs of the
/// subgraph they induce, in forward topological order.
///
/// Within each returned SCC, members keep their order in `members`.
fn split_scc<N, E, Ix: IndexType>(
    graph: &Graph<N, E, Directed, Ix>,
    members: &[NodeIndex<Ix>],
) -> Vec<Vec<NodeIndex<Ix>>> {
    let local_ixs: AHashMap<NodeIndex<Ix>, usize> = members
        .iter()
        .enumerate()
        .map(|(local_ix, ix)| (*ix, local_ix))
        .collect();
    let mut subgraph = Graph::<(), (), Directed, usize>::with_capacity(members.len(), 0);
    for _ in members {
        subgraph.add_node(());
    }
    for (from_local, from) in members.iter().enumerate() {
        for to in graph.neighbors_directed(*from, Outgoing) {
            if let Some(&to_local) = local_ixs.get(&to) {
                subgraph.add_edge(NodeIndex::new(from_local), NodeIndex::new(to_local), ());
            }
        }
    }

    // kosaraju_scc returns SCCs in reverse topological order, so reverse it
    // again.
    kosaraju_scc(&subgraph)
        .into_iter()
        .rev()
        .map(|mut sub_scc| {
            sub_scc.sort_unstable();
            sub_scc
                .into_iter()
                .map(|local_ix| members[local_ix.index()])
                .collect()
        })
        .collect()
}

/// An iterator over the nodes of strongly connected components.
///
/// This is kept small because each call that iterates over a set (such as
/// `package_ids`) moves it by value several times.
#[derive(Clone, Debug)]
pub(crate) enum NodeIter<'a, Ix: IndexType, F> {
    Walk {
        iter: slice::Iter<'a, NodeIndex<Ix>>,
        included: F,
        direction: Direction,
    },
    Sorted {
        iter: vec::IntoIter<(usize, NodeIndex<Ix>)>,
        direction: Direction,
    },
}

impl<Ix: IndexType, F: FilterNode<NodeIndex<Ix>>> Iterator for NodeIter<'_, Ix, F> {
    type Item = NodeIndex<Ix>;

    fn next(&mut self) -> Option<NodeIndex<Ix>> {
        // Note that outgoing implies iterating over the sccs in forward order, while incoming means
        // sccs in reverse order.
        match self {
            NodeIter::Walk {
                iter,
                included,
                direction,
            } => match direction {
                Direction::Outgoing => loop {
                    let ix = *iter.next()?;
                    if included.include_node(ix) {
                        return Some(ix);
                    }
                },
                Direction::Incoming => loop {
                    let ix = *iter.next_back()?;
                    if included.include_node(ix) {
                        return Some(ix);
                    }
                },
            },
            NodeIter::Sorted { iter, direction } => match direction {
                Direction::Outgoing => iter.next().map(|(_, ix)| ix),
                Direction::Incoming => iter.next_back().map(|(_, ix)| ix),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use petgraph::{
        Graph,
        visit::{NodeFiltered, Reversed},
    };
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
        let all: FixedBitSet = graph.node_indices().map(|ix| ix.index()).collect();
        let externals: HashSet<NodeIndex<u32>> = sccs
            .for_subset(&graph, &all)
            .externals(&graph, &all, Direction::Outgoing)
            .collect();
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
        let all: FixedBitSet = graph.node_indices().map(|ix| ix.index()).collect();
        let externals: HashSet<NodeIndex<u32>> = sccs
            .for_subset(&graph, &all)
            .externals(&graph, &all, Direction::Outgoing)
            .collect();
        assert_eq!(externals, HashSet::from([a, b]));
    }

    #[test]
    fn for_subset_only_splits_partially_included_sccs() {
        // Here, we define an SCC a -> b -> c -> a, plus d outside the cycle.
        let mut graph = Graph::<(), (), Directed, u32>::new();
        let a = graph.add_node(());
        let b = graph.add_node(());
        let c = graph.add_node(());
        let d = graph.add_node(());
        graph.add_edge(a, b, ());
        graph.add_edge(b, c, ());
        graph.add_edge(c, a, ());
        graph.add_edge(c, d, ());

        let sccs = Sccs::<u32>::new(&graph, |_| {});
        // These are all possible SCCs.
        for included in [vec![a, b, c], vec![a, b, c, d], vec![d], vec![]] {
            let included: FixedBitSet = included.iter().map(|ix| ix.index()).collect();
            assert!(
                !sccs.for_subset(&graph, &included).is_split(),
                "no SCC is split by {included:?}",
            );
        }
    }

    /// Test that when a set breaks an SCC but a smaller cycle remains, that
    /// cycle's members keep the non-dev order the SCC sorter gave the original
    /// SCC.
    #[test]
    fn for_subset_keeps_inner_cycle_order() {
        // Here, we make an SCC {a, b, c, d} via a -> b -> c -> a and b <-> d.
        // Removing a leaves cycle {b, d} followed by c.
        let mut graph = Graph::<(), (), Directed, u32>::new();
        let a = graph.add_node(());
        let b = graph.add_node(());
        let c = graph.add_node(());
        let d = graph.add_node(());
        graph.add_edge(a, b, ());
        graph.add_edge(b, c, ());
        graph.add_edge(c, a, ());
        graph.add_edge(b, d, ());
        graph.add_edge(d, b, ());

        // Reverse index order, so that keeping the sorter's order can be
        // distinguished from sorting by index.
        let sccs = Sccs::<u32>::new(&graph, |scc| scc.sort_by(|x, y| y.cmp(x)));
        let included: FixedBitSet = [b, c, d].iter().map(|ix| ix.index()).collect();
        let subset = sccs.for_subset(&graph, &included);
        assert!(subset.is_split(), "removing a splits the SCC");
        assert!(subset.is_same_scc(b, d), "b and d still form a cycle");
        assert!(!subset.is_same_scc(b, c), "c is split from the cycle");

        let forward: Vec<_> = subset.node_iter(&included, Direction::Outgoing).collect();
        assert_eq!(forward, vec![d, b, c]);

        let roots: Vec<_> = subset
            .externals(
                &NodeFiltered(&graph, &included),
                &included,
                Direction::Outgoing,
            )
            .collect();
        assert_eq!(roots, vec![d, b]);
        let reverse_roots: Vec<_> = subset
            .externals(
                &NodeFiltered(Reversed(&graph), &included),
                &included,
                Direction::Incoming,
            )
            .collect();
        assert_eq!(reverse_roots, vec![c]);
    }

    impl NodeSet<u32> for &FixedBitSet {
        type Members = vec::IntoIter<NodeIndex<u32>>;

        fn member_count(&self) -> usize {
            self.count_ones(..)
        }

        fn members(&self) -> Self::Members {
            self.ones()
                .map(NodeIndex::new)
                .collect::<Vec<_>>()
                .into_iter()
        }
    }
}
