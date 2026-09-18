// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Support for weak features.
//!
//! A weak feature such as `a = ["foo?/std"]` is a single edge in the feature
//! graph, but Cargo applies it to each declaration of `foo` separately. guppy
//! models that by splitting the edge's link into two halves, and running the
//! traversal through the buffered edge filter in
//! [`crate::petgraph_support::dfs`]:
//!
//! * The half covering `foo`'s required declarations is offered to the visitor
//!   as soon as the edge is reached. It is absent if `foo` has no required
//!   declaration.
//! * The half covering `foo`'s optional declarations is held back in a buffer,
//!   and offered to the visitor when that buffer is released.
//!
//! The edge is followed if the visitor accepts either half.

use crate::graph::{
    PackageIx,
    feature::{ConditionalLink, EdgeLinks, FeatureEdgeReference},
};
use indexmap::IndexSet;
use itertools::Either;
use petgraph::graph::EdgeIndex;
use smallvec::SmallVec;

/// Data structure that tracks pairs of package indexes that form weak dependencies.
#[derive(Clone, Debug)]
pub(super) struct WeakDependencies {
    ixs: IndexSet<EdgeIndex<PackageIx>>,
}

impl WeakDependencies {
    pub(super) fn new() -> Self {
        Self {
            ixs: IndexSet::new(),
        }
    }

    pub(super) fn insert(&mut self, edge_ix: EdgeIndex<PackageIx>) -> WeakIndex {
        WeakIndex(self.ixs.insert_full(edge_ix).0)
    }

    pub(super) fn get(&self, edge_ix: EdgeIndex<PackageIx>) -> Option<WeakIndex> {
        self.ixs.get_index_of(&edge_ix).map(WeakIndex)
    }

    #[inline]
    pub(super) fn new_buffer_states<'g, F>(&self, accept_fn: F) -> WeakBufferStates<'g, '_, F>
    where
        F: FnMut(ConditionalLink<'g>) -> bool,
    {
        WeakBufferStates::new(self, self.ixs.len(), accept_fn)
    }
}

// Not part of the public API -- exposed for testing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[doc(hidden)]
pub struct WeakIndex(pub(super) usize);

/// Buffer states for weak indexes, to be used during a feature resolver traversal.
pub(super) struct WeakBufferStates<'g, 'a, F> {
    deps: &'a WeakDependencies,
    states: SmallVec<[SingleBufferState<'g>; 8]>,
    accept_fn: F,
}

impl<'g, 'a, F> WeakBufferStates<'g, 'a, F>
where
    F: FnMut(ConditionalLink<'g>) -> bool,
{
    #[inline]
    fn new(deps: &'a WeakDependencies, len: usize, accept_fn: F) -> Self {
        let mut states = SmallVec::with_capacity(len);
        states.resize_with(len, || SingleBufferState::Buffered(SingleBufferVec::new()));
        Self {
            deps,
            states,
            accept_fn,
        }
    }

    pub(super) fn track(
        &mut self,
        edge_ref: FeatureEdgeReference<'g>,
        links: EdgeLinks<'g>,
    ) -> Either<Option<FeatureEdgeReference<'g>>, Vec<FeatureEdgeReference<'g>>> {
        match links {
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
                // the optional half. It would never reach the buffer, and the
                // visitor would not see it once the buffer is released.
                //
                // The optional half is buffered along with `edge_ref` even when
                // the required half was just accepted, so releasing the buffer
                // can hand the same `edge_ref` to the traversal a second time.
                // That is harmless: the DFS checks its discovered set before
                // pushing a target.
                let required_accepted = required.is_some_and(|required| (self.accept_fn)(required));
                let optional_accepted = match &mut self.states[index.0] {
                    SingleBufferState::Buffered(buffer) => {
                        // The buffer has not been released yet.
                        buffer.push((optional, edge_ref));
                        false
                    }
                    SingleBufferState::Released => {
                        // The buffer has already been released.
                        (self.accept_fn)(optional)
                    }
                };
                Either::Left((required_accepted || optional_accepted).then_some(edge_ref))
            }
            EdgeLinks::NonWeak(link) => {
                if !(self.accept_fn)(link) {
                    // This link was not accepted -- ignore its presence.
                    return Either::Left(None);
                }

                // A link derived from several package edges releases the buffer
                // corresponding to all of them.
                let mut released: Vec<FeatureEdgeReference<'g>> = Vec::new();
                for package_edge_ix in link.package_edge_ixs().iter() {
                    let Some(weak_index) = self.deps.get(package_edge_ix) else {
                        // Not a weak link.
                        continue;
                    };
                    match std::mem::replace(
                        &mut self.states[weak_index.0],
                        SingleBufferState::Released,
                    ) {
                        SingleBufferState::Buffered(buffer) => {
                            // Transition from buffered to released.
                            released.extend(buffer.into_iter().filter_map(|(link, edge_ref)| {
                                // Filter buffered links.
                                (self.accept_fn)(link).then_some(edge_ref)
                            }));
                        }
                        SingleBufferState::Released => {
                            // Weak link, but the buffer is already released.
                        }
                    }
                }

                if released.is_empty() {
                    Either::Left(Some(edge_ref))
                } else {
                    released.push(edge_ref);
                    Either::Right(released)
                }
            }
        }
    }
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
