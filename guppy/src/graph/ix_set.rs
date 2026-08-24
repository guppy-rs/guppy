// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::graph::GraphSpec;
use fixedbitset::{FixedBitSet, Ones};
use petgraph::{graph::NodeIndex, visit::FilterNode};
use std::{fmt, marker::PhantomData};

/// A dense bitset of node indexes.
///
/// The capacity is always the same as the node count, and `len` is a cache of
/// `bits.count_ones(..)`.
pub(super) struct IxSet<G> {
    bits: FixedBitSet,
    len: usize,
    _phantom: PhantomData<G>,
}

// This manual impl avoids a G: Clone bound.
impl<G> Clone for IxSet<G> {
    fn clone(&self) -> Self {
        Self {
            bits: self.bits.clone(),
            len: self.len,
            _phantom: PhantomData,
        }
    }
}

impl<G: GraphSpec> IxSet<G> {
    pub(super) fn empty(node_count: usize) -> Self {
        Self {
            bits: FixedBitSet::with_capacity(node_count),
            len: 0,
            _phantom: PhantomData,
        }
    }

    pub(super) fn from_visit_map(bits: FixedBitSet, len: usize, node_count: usize) -> Self {
        debug_assert_eq!(
            bits.len(),
            node_count,
            "visit map capacity matches the graph's node count"
        );
        debug_assert_eq!(bits.count_ones(..), len, "visit map popcount matches len");
        Self {
            bits,
            len,
            _phantom: PhantomData,
        }
    }

    pub(super) fn from_ixs(
        ixs: impl IntoIterator<Item = NodeIndex<G::Ix>>,
        node_count: usize,
    ) -> Self {
        let mut bits = FixedBitSet::with_capacity(node_count);
        let mut len = 0;
        for ix in ixs {
            debug_assert!(
                ix.index() < node_count,
                "node index {} is within the graph's node count {}",
                ix.index(),
                node_count
            );
            if !bits.put(ix.index()) {
                len += 1;
            }
        }
        Self {
            bits,
            len,
            _phantom: PhantomData,
        }
    }

    pub(super) fn from_bits(mut bits: FixedBitSet, node_count: usize) -> Self {
        debug_assert!(
            bits.len() <= node_count,
            "bits capacity is at most the graph's node count"
        );
        bits.grow(node_count);
        let len = bits.count_ones(..);
        Self {
            bits,
            len,
            _phantom: PhantomData,
        }
    }

    pub(super) fn len(&self) -> usize {
        self.len
    }

    pub(super) fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub(super) fn contains(&self, ix: NodeIndex<G::Ix>) -> bool {
        self.bits.contains(ix.index())
    }

    pub(super) fn ones(&self) -> IxOnes<'_, G> {
        IxOnes {
            ones: self.bits.ones(),
            remaining: self.len,
            _phantom: PhantomData,
        }
    }

    pub(super) fn union_with(&mut self, other: &Self) {
        self.bits.union_with(&other.bits);
        self.recount();
    }

    pub(super) fn intersect_with(&mut self, other: &Self) {
        self.bits.intersect_with(&other.bits);
        self.recount();
    }

    pub(super) fn difference(&self, other: &Self) -> Self {
        let mut res = self.clone();
        res.bits.difference_with(&other.bits);
        res.recount();
        res
    }

    pub(super) fn symmetric_difference_with(&mut self, other: &Self) {
        self.bits.symmetric_difference_with(&other.bits);
        self.recount();
    }

    fn recount(&mut self) {
        self.len = self.bits.count_ones(..);
    }
}

// A manual impl avoids a G: Debug bound and prints indexes.
impl<G: GraphSpec> fmt::Debug for IxSet<G> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("IxSet ")?;
        f.debug_set().entries(self.ones()).finish()
    }
}

// This manual impl avoids a G: PartialEq bound.
impl<G: GraphSpec> PartialEq for IxSet<G> {
    fn eq(&self, other: &Self) -> bool {
        self.len == other.len && self.bits == other.bits
    }
}

impl<G: GraphSpec> Eq for IxSet<G> {}

/// A ones iterator on an [`IxSet`].
pub(super) struct IxOnes<'a, G> {
    ones: Ones<'a>,
    remaining: usize,
    _phantom: PhantomData<G>,
}

impl<G: GraphSpec> Iterator for IxOnes<'_, G> {
    type Item = NodeIndex<G::Ix>;

    fn next(&mut self) -> Option<Self::Item> {
        let ix = self.ones.next()?;
        self.remaining -= 1;
        Some(NodeIndex::new(ix))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<G: GraphSpec> ExactSizeIterator for IxOnes<'_, G> {
    fn len(&self) -> usize {
        self.remaining
    }
}

impl<G: GraphSpec> FilterNode<NodeIndex<G::Ix>> for &IxSet<G> {
    fn include_node(&self, node: NodeIndex<G::Ix>) -> bool {
        self.contains(node)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    enum TestSpec {}

    impl GraphSpec for TestSpec {
        type Node = ();
        type Edge = ();
        type Ix = u32;
    }

    fn make_set(ixs: impl IntoIterator<Item = usize>, node_count: usize) -> IxSet<TestSpec> {
        IxSet::from_ixs(ixs.into_iter().map(NodeIndex::new), node_count)
    }

    #[test]
    fn from_ixs_dedups() {
        let set = make_set([1, 3, 1, 2, 3], 5);
        assert_eq!(set.len(), 3, "len matches the number of distinct indices");
        let ixs: Vec<_> = set.ones().map(|ix| ix.index()).collect();
        assert_eq!(ixs, [1, 2, 3], "ones yields distinct indices in order");
    }

    #[test]
    fn ix_ones_exact_size() {
        let set = make_set([0, 2, 4], 6);
        let mut iter = set.ones();
        assert_eq!(iter.len(), 3, "full iterator reports len 3");
        assert_eq!(iter.size_hint(), (3, Some(3)), "size_hint matches len");
        assert_eq!(
            iter.next().map(NodeIndex::index),
            Some(0),
            "first index is 0"
        );
        assert_eq!(iter.len(), 2, "len decreases after partial consumption");
        assert_eq!(iter.size_hint(), (2, Some(2)), "size_hint matches len");
        assert_eq!(
            iter.by_ref().map(NodeIndex::index).collect::<Vec<_>>(),
            [2, 4],
            "remaining indices are 2 and 4"
        );
        assert_eq!(iter.len(), 0, "exhausted iterator reports len 0");
        assert_eq!(
            iter.size_hint(),
            (0, Some(0)),
            "exhausted size_hint is empty"
        );
        assert_eq!(iter.next(), None, "exhausted iterator yields None");
    }
}
