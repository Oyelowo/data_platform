//! Join specification for the Yelang VM.
//!
//! A [`JoinSpec`] describes how to combine two query results. The VM's
//! `QueryJoin` instruction uses it to execute the join: an equi-join (one or
//! more pairs of key columns) runs as a hash build/probe, while a join with no
//! resolvable keys (or a [`JoinKind::Cross`]) falls back to a nested loop.
//!
//! # Model
//!
//! Given a left and a right row collection:
//!
//! 1. For an equi-join, the join key of a row is the tuple of its `left_keys`
//!    (left side) or `right_keys` (right side) field values. Two rows *match*
//!    when their key tuples are equal.
//! 2. A hash join builds a table keyed by the right-side key tuple, then probes
//!    it with each left row's key. A nested-loop join compares every pair.
//! 3. Matched row pairs are merged into a single output row (left fields first,
//!    then any right fields whose names do not collide). The [`JoinKind`]
//!    decides which rows survive (inner, outer, semi, anti, cross).

use yelang_interner::Symbol;

/// The set-algebraic join type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinKind {
    /// Keep only row pairs that match.
    Inner,
    /// Keep every left row; unmatched left rows are padded with null right
    /// columns.
    Left,
    /// Keep every right row; unmatched right rows are padded with null left
    /// columns.
    Right,
    /// Keep every row from both sides; unmatched rows on either side are padded
    /// with nulls.
    Full,
    /// Keep left rows that have at least one match (no right columns emitted).
    Semi,
    /// Keep left rows that have no match (no right columns emitted).
    Anti,
    /// Cartesian product: every left/right pair, no predicate.
    Cross,
}

/// How the VM physically executes a join.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinAlgorithm {
    /// Build a hash table on the right side keyed by the join key, probe with
    /// the left. Requires at least one pair of equi-join keys.
    Hash,
    /// Compare every left/right row pair. Used for cross joins, non-equi joins,
    /// or whenever no join keys are available.
    NestedLoop,
}

/// A specification for joining two query results.
#[derive(Debug, Clone, PartialEq)]
pub struct JoinSpec {
    /// The join type.
    pub kind: JoinKind,
    /// The physical execution strategy.
    pub algorithm: JoinAlgorithm,
    /// Left-side join key column names (one per equi-join predicate).
    pub left_keys: Vec<Symbol>,
    /// Right-side join key column names, aligned with [`JoinSpec::left_keys`].
    pub right_keys: Vec<Symbol>,
}

impl JoinSpec {
    /// Whether this spec describes an equi-join with a usable key on each side.
    pub fn is_equi(&self) -> bool {
        !self.left_keys.is_empty() && self.left_keys.len() == self.right_keys.len()
    }
}
