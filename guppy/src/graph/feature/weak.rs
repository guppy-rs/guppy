// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Support for weak features.
//!
//! A weak feature such as `a = ["foo?/std"]` is a single edge in the feature
//! graph for each package `foo` resolves to (usually one), but Cargo applies
//! it to each declaration of `foo` separately. guppy models that by splitting
//! each edge's link into two halves, and running the traversal through the
//! buffered edge filter in [`crate::petgraph_support::dfs`]:
//!
//! * The half covering `foo`'s required declarations is offered to the visitor
//!   as soon as the edge is reached. It is absent if `foo` has no required
//!   declaration.
//! * The half covering `foo`'s optional declarations is held in a buffer in a
//!   forward query until `dep:foo` is activated, and offered then. If
//!   `dep:foo` is never reached, that half is never offered. In a reverse
//!   query, it is offered as soon as the edge is reached.
//!
//! The edge is followed if the visitor accepts either half.

use crate::graph::{
    DependencyDirection, FeatureIx, PackageIx,
    feature::{ConditionalLink, EdgeLinks, FeatureEdgeReference},
};
use ahash::AHashMap;
use indexmap::IndexSet;
use petgraph::graph::{EdgeIndex, NodeIndex};
use smallvec::SmallVec;

/// Data structure that tracks pairs of package indexes that form weak dependencies.
#[derive(Clone, Debug)]
pub(super) struct WeakDependencies {
    ixs: IndexSet<EdgeIndex<PackageIx>>,
    // A map of dep:foo node to the weak indexes it releases when reached.
    by_optional_dependency: AHashMap<NodeIndex<FeatureIx>, SmallVec<[WeakIndex; 1]>>,
}

impl WeakDependencies {
    pub(super) fn new() -> Self {
        Self {
            ixs: IndexSet::new(),
            by_optional_dependency: AHashMap::new(),
        }
    }

    pub(super) fn insert(
        &mut self,
        edge_ix: EdgeIndex<PackageIx>,
        optional_dependency_ix: NodeIndex<FeatureIx>,
    ) -> WeakIndex {
        let (index, inserted) = self.ixs.insert_full(edge_ix);
        let index = WeakIndex(index);
        if inserted {
            self.by_optional_dependency
                .entry(optional_dependency_ix)
                .or_default()
                .push(index);
        }
        index
    }
}

// Not part of the public API -- exposed for testing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[doc(hidden)]
pub struct WeakIndex(pub(super) usize);

/// Buffer states for weak indexes, to be used during a feature resolver traversal.
pub(super) struct WeakBufferStates<'g, 'a, F> {
    buffers: WeakBuffers<'g, 'a>,
    accept_fn: F,
}

/// The buffers a traversal keeps for the optional halves of weak edges.
enum WeakBuffers<'g, 'a> {
    /// Buffering is enabled. Used by forward queries.
    ///
    /// A buffer is released when the traversal discovers `dep:foo`, which is
    /// when the dependent activates `foo`'s optional declarations.
    PerPackageEdge {
        /// The weak dependencies that map a `dep:foo` node back to the indexes
        /// it releases.
        deps: &'a WeakDependencies,

        /// A buffer for each weak index.
        states: Vec<SingleBufferState<'g>>,
    },

    /// No weak buffers: optional links are offered as soon as they are reached.
    /// Used by reverse queries.
    Unbuffered,
}

impl<'g, 'a, F> WeakBufferStates<'g, 'a, F>
where
    F: FnMut(ConditionalLink<'g>) -> bool,
{
    /// Returns buffer states for a traversal in `direction`.
    #[inline]
    pub(super) fn new(
        deps: &'a WeakDependencies,
        direction: DependencyDirection,
        accept_fn: F,
    ) -> Self {
        let buffers = match direction {
            DependencyDirection::Forward => {
                let len = deps.ixs.len();
                let mut states = Vec::with_capacity(len);
                states.resize_with(len, || SingleBufferState::Buffered(SingleBufferVec::new()));
                WeakBuffers::PerPackageEdge { deps, states }
            }
            DependencyDirection::Reverse => WeakBuffers::Unbuffered,
        };
        Self { buffers, accept_fn }
    }

    pub(super) fn track(
        &mut self,
        edge_ref: FeatureEdgeReference<'g>,
        links: EdgeLinks<'g>,
    ) -> Option<FeatureEdgeReference<'g>> {
        let accepted = match links {
            EdgeLinks::Weak {
                required,
                optional,
                index,
            } => {
                // Handle both halves, required first.
                //
                // Both halves must be dealt with before they are combined, so
                // don't fold these into a single `a || b` expression. If the
                // required half is accepted, a short-circuiting `||` would skip
                // the optional half, and the visitor would never see it.
                //
                // A buffered optional half is held along with `edge_ref` even
                // when the required half was just accepted, so releasing the
                // buffer can hand the same `edge_ref` to the traversal a second
                // time. That is harmless: the DFS checks its discovered set
                // before pushing a target.
                let required_accepted = required.is_some_and(|required| (self.accept_fn)(required));
                let optional_accepted = match &mut self.buffers {
                    WeakBuffers::PerPackageEdge { deps: _, states } => match &mut states[index.0] {
                        SingleBufferState::Buffered(buffer) => {
                            buffer.push((optional, edge_ref));
                            false
                        }
                        SingleBufferState::Released => (self.accept_fn)(optional),
                    },
                    WeakBuffers::Unbuffered => (self.accept_fn)(optional),
                };
                required_accepted || optional_accepted
            }
            EdgeLinks::NonWeak(link) => (self.accept_fn)(link),
        };
        accepted.then_some(edge_ref)
    }

    // Called when the DFS reaches a feature node.
    pub(super) fn discover(
        &mut self,
        feature_ix: NodeIndex<FeatureIx>,
    ) -> Vec<FeatureEdgeReference<'g>> {
        let (deps, states) = match &mut self.buffers {
            WeakBuffers::PerPackageEdge { deps, states } => (*deps, states),
            WeakBuffers::Unbuffered => return Vec::new(),
        };
        let Some(weak_indexes) = deps.by_optional_dependency.get(&feature_ix) else {
            return Vec::new();
        };

        release_buffers(states, weak_indexes, &mut self.accept_fn)
    }
}

fn release_buffers<'g, F>(
    states: &mut [SingleBufferState<'g>],
    weak_indexes: &[WeakIndex],
    accept_fn: &mut F,
) -> Vec<FeatureEdgeReference<'g>>
where
    F: FnMut(ConditionalLink<'g>) -> bool,
{
    let mut released = Vec::new();
    for weak_index in weak_indexes {
        match std::mem::replace(&mut states[weak_index.0], SingleBufferState::Released) {
            SingleBufferState::Buffered(buffer) => {
                // Transition from buffered to released.
                released.extend(buffer.into_iter().filter_map(|(link, edge_ref)| {
                    // Filter buffered links.
                    accept_fn(link).then_some(edge_ref)
                }));
            }
            SingleBufferState::Released => {
                // This should never happen, since nodes are discovered at
                // most once.
                debug_assert!(false, "weak index {weak_index:?} is released once");
            }
        }
    }
    released
}

/// Buffer state for a single weak index in an in-progress resolver.
enum SingleBufferState<'g> {
    /// The optional halves seen so far, held until the buffer is released.
    Buffered(SingleBufferVec<'g>),

    /// The buffer has been released: optional halves are offered to the
    /// visitor as they are reached.
    Released,
}

type SingleBufferVec<'g> = Vec<(ConditionalLink<'g>, FeatureEdgeReference<'g>)>;
