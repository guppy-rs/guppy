// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::graph::GraphSpec;
use fixedbitset::FixedBitSet;
use petgraph::{graph::NodeIndex, visit::FilterNode};
use std::marker::PhantomData;

/// A dense bitset of node indexes.
///
/// The capacity is always the same as the node count, and `len` is a cache of
/// `bits.count_ones(..)`.
#[derive(Debug)]
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

    pub(super) fn ones(&self) -> impl Iterator<Item = NodeIndex<G::Ix>> + '_ {
        self.bits.ones().map(NodeIndex::new)
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

// This manual impl avoids a G: PartialEq bound.
impl<G: GraphSpec> PartialEq for IxSet<G> {
    fn eq(&self, other: &Self) -> bool {
        self.len == other.len && self.bits == other.bits
    }
}

impl<G: GraphSpec> Eq for IxSet<G> {}

impl<G: GraphSpec> FilterNode<NodeIndex<G::Ix>> for &IxSet<G> {
    fn include_node(&self, node: NodeIndex<G::Ix>) -> bool {
        self.contains(node)
    }
}
