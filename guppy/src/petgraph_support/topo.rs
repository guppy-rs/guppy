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

        // A node is a root iff it has no incoming neighbors *other than
        // itself* -- a self-loop is internal to the node's own (single-
        // element) SCC and must not disqualify it from being a root. This
        // matches `Sccs::externals`'s single-node SCC branch in
        // `petgraph_support::scc`.
        let roots = graph
            .node_identifiers()
            .filter(move |&a| !graph.neighbors_directed(a, Incoming).any(|n| n != a));
        dfs.stack.extend(roots);

        let mut topo: Vec<NodeIndex<Ix>> = (&mut dfs).iter(graph).collect();
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

        // topo.len cannot possibly exceed graph.node_count().
        assert!(
            topo.len() <= graph.node_count(),
            "topo.len() <= graph.node_count() ({} is actually > {})",
            topo.len(),
            graph.node_count(),
        );
        if topo.len() < graph.node_count() {
            // This means there was a multi-node cycle in the graph which caused some nodes to be
            // skipped: none of its members appears as a root (each has a non-self incoming edge),
            // so the DFS never starts inside it. (Self-loops on otherwise-root nodes are handled
            // by the root predicate above, matching `Sccs::externals`.)
            //
            // In this case, do a best-effort job: fill in the missing nodes with their reverse
            // index set to the end of the topo order. We could do something fancier here with sccs,
            // but for guppy this should never happen in practice. (In fact, the one time this code
            // was hit there was actually an underlying bug.)
            let mut next = topo.len();
            for n in 0..graph.node_count() {
                let a = NodeIndex::new(n);
                if !dfs.finished.is_visited(&a) {
                    // a is a missing index.
                    reverse_index[a.index()] = next;
                    next += 1;
                }
            }
        }

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

#[cfg(test)]
mod tests {
    use super::*;
    use petgraph::Graph;

    /// A self-loop on a node with no other incoming edges must not
    /// disqualify it from being a root. Without the fix, the node was
    /// filtered out of the root set, its descendants were never visited
    /// by the DFS, and the best-effort fallback placed them in node-
    /// insertion order -- which can disagree with topological order.
    #[test]
    fn topo_self_loop_root_orders_descendants_correctly() {
        // Insert `b` (index 0) before `a` (index 1) so that node-index
        // order *disagrees* with topological order: the edge `a -> b`
        // means `a` should precede `b`.
        let mut graph = Graph::<(), (), Directed, u32>::new();
        let b = graph.add_node(());
        let a = graph.add_node(());
        graph.add_edge(a, b, ());
        graph.add_edge(a, a, ());

        let topo = TopoWithCycles::<u32>::new(&graph);
        assert!(
            topo.topo_ix(a) < topo.topo_ix(b),
            "a should precede b in topo order despite the self-loop on a \
             (got topo_ix(a)={}, topo_ix(b)={})",
            topo.topo_ix(a),
            topo.topo_ix(b),
        );
    }

    /// A self-loop on a node that is *also* reachable from a real root
    /// must not change anything: the existing root drives the DFS and
    /// the self-loop is ignored.
    #[test]
    fn topo_self_loop_on_non_root_is_harmless() {
        // b -> a, plus a self-loop on a. `b` is the only root; `a` is
        // visited via b's outgoing edge.
        let mut graph = Graph::<(), (), Directed, u32>::new();
        let a = graph.add_node(());
        let b = graph.add_node(());
        graph.add_edge(b, a, ());
        graph.add_edge(a, a, ());

        let topo = TopoWithCycles::<u32>::new(&graph);
        assert!(
            topo.topo_ix(b) < topo.topo_ix(a),
            "b should precede a in topo order (got topo_ix(b)={}, topo_ix(a)={})",
            topo.topo_ix(b),
            topo.topo_ix(a),
        );
    }
}

#[cfg(all(test, feature = "proptest1"))]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn graph_topo_sort(graph in possibly_cyclic_graph()) {
            let topo = TopoWithCycles::new(&graph);
            let mut nodes: Vec<_> = graph.node_indices().collect();

            check_consistency(&topo, graph.node_count());

            topo.sort_nodes(&mut nodes);
            for (topo_ix, node_ix) in nodes.iter().enumerate() {
                assert_eq!(topo.topo_ix(*node_ix), topo_ix);
            }

        }
    }

    fn possibly_cyclic_graph() -> impl Strategy<Value = Graph<(), ()>> {
        // Generate a graph in adjacency list form. N nodes, up to N**2 edges.
        (1..=100usize)
            .prop_flat_map(|n| {
                (
                    Just(n),
                    prop::collection::vec(prop::collection::vec(0..n, 0..n), n),
                )
            })
            .prop_map(|(n, adj)| {
                let mut graph =
                    Graph::<(), ()>::with_capacity(n, adj.iter().map(|x| x.len()).sum());
                for _ in 0..n {
                    // Add all the nodes under consideration.
                    graph.add_node(());
                }
                for (src, dsts) in adj.into_iter().enumerate() {
                    let src = NodeIndex::new(src);
                    for dst in dsts {
                        let dst = NodeIndex::new(dst);
                        graph.update_edge(src, dst, ());
                    }
                }
                graph
            })
    }

    fn check_consistency(topo: &TopoWithCycles<u32>, n: usize) {
        // Ensure that all indexes are covered and unique.
        let mut seen = vec![false; n];
        for i in 0..n {
            let topo_ix = topo.topo_ix(NodeIndex::new(i));
            assert!(
                !seen[topo_ix],
                "topo_ix {topo_ix} should be seen exactly once, but seen twice"
            );
            seen[topo_ix] = true;
        }
        for (i, &this_seen) in seen.iter().enumerate() {
            assert!(this_seen, "topo_ix {i} should be seen, but wasn't");
        }
    }
}
