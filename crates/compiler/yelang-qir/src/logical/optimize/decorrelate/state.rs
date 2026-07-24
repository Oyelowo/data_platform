//! Unnesting state for dependent join elimination (BTW 2025 Fig 4).
//!
//! Split into two parts per the paper:
//! - [`UnnestingInfo`]: global, shared across the tree walk for one DJoin.
//! - [`Unnesting`]: per-fragment, changes as we walk the tree.

use yelang_arena::FxHashMap;
use yelang_interner::Symbol;

use crate::logical::plan::PlanId;

use super::union_find::UnionFind;

// ---------------------------------------------------------------------------
// UnnestingInfo — global, immutable during one unnesting pass
// ---------------------------------------------------------------------------

/// Global info for one dependent join being eliminated.
///
/// Shared (via index) across all [`Unnesting`] frames created during
/// the walk for this dependent join.
#[derive(Debug)]
pub(super) struct UnnestingInfo {
    /// The `DependentJoin` node being eliminated.
    pub(super) join_id: PlanId,
    /// A(outer) ∩ F(inner): outer columns accessed by the right side.
    pub(super) outer_refs: Vec<Symbol>,
    /// Domain computation: Π_{outer_refs}(outer). Built lazily.
    pub(super) domain: Option<PlanId>,
    /// Parent unnesting info index (for nested dependent joins).
    pub(super) parent: Option<usize>,
}

// ---------------------------------------------------------------------------
// Unnesting — per-fragment, mutable during the walk
// ---------------------------------------------------------------------------

/// Per-fragment unnesting state.
///
/// One instance per tree fragment being unnested. Created fresh when
/// we split at a join (both sides get their own Unnesting), or inherited
/// from the parent when we recurse into a linear operator.
#[derive(Debug)]
pub(super) struct Unnesting {
    /// Index into [`UnnestingState::infos`] for the global info.
    pub(super) info_idx: usize,
    /// Union-find of equivalent columns (from predicates encountered so far).
    pub(super) cclasses: UnionFind,
    /// Substitution map: outer_ref → local column.
    /// Populated when we decide to substitute instead of domain-join.
    pub(super) repr: FxHashMap<Symbol, Symbol>,
}

impl Unnesting {
    pub(super) fn new(info_idx: usize) -> Self {
        Self {
            info_idx,
            cclasses: UnionFind::new(),
            repr: FxHashMap::default(),
        }
    }

    /// Get the global info for this unnesting.
    pub(super) fn info<'a>(&self, state: &'a UnnestingState) -> &'a UnnestingInfo {
        &state.infos[self.info_idx]
    }

    /// Get mutable global info.
    pub(super) fn info_mut<'a>(&mut self, state: &'a mut UnnestingState) -> &'a mut UnnestingInfo {
        &mut state.infos[self.info_idx]
    }

    /// Whether all outer refs have a substitution in repr.
    pub(super) fn all_substitutable(&self, state: &UnnestingState) -> bool {
        let info = self.info(state);
        info.outer_refs.iter().all(|r| self.repr.contains_key(r))
    }

    /// Merge equivalences and substitutions from another Unnesting
    /// (used after unnesting both sides of a join).
    pub(super) fn merge(&mut self, other: &Unnesting) {
        self.cclasses.merge(&other.cclasses);
        for (k, v) in &other.repr {
            self.repr.entry(*k).or_insert(*v);
        }
    }
}

// ---------------------------------------------------------------------------
// AccessingAnnotation — Phase 1 result
// ---------------------------------------------------------------------------

/// Phase 1 annotation: which operators access a dependent join's left side.
///
/// Built by [`super::annotate::annotate_accessing`].
#[derive(Debug, Default)]
pub(super) struct AccessingAnnotation {
    /// Map from DependentJoin PlanId → set of accessing operator PlanIds.
    pub(super) accessing: FxHashMap<PlanId, Vec<PlanId>>,
}

impl AccessingAnnotation {
    pub(super) fn new() -> Self {
        Self {
            accessing: FxHashMap::default(),
        }
    }

    /// Get the accessing operators for a dependent join.
    pub(super) fn accessing(&self, join_id: PlanId) -> &[PlanId] {
        self.accessing.get(&join_id).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Whether a dependent join is trivial (no accessing operators).
    pub(super) fn is_trivial(&self, join_id: PlanId) -> bool {
        self.accessing.get(&join_id).map_or(true, |v| v.is_empty())
    }

    /// Add an accessing operator for a dependent join.
    pub(super) fn add(&mut self, join_id: PlanId, op_id: PlanId) {
        self.accessing.entry(join_id).or_default().push(op_id);
    }
}

// ---------------------------------------------------------------------------
// UnnestingState — the full state stack
// ---------------------------------------------------------------------------

/// The full unnesting state: stack of Unnesting frames + global infos.
pub(super) struct UnnestingState {
    /// Global infos (one per dependent join being eliminated).
    pub(super) infos: Vec<UnnestingInfo>,
    /// Stack of active Unnesting frames.
    pub(super) stack: Vec<Unnesting>,
    /// Phase 1 annotations.
    pub(super) annotations: AccessingAnnotation,
    /// CTE DAG cutting: maps original PlanId → decorrelated PlanId.
    pub(super) cache: FxHashMap<PlanId, PlanId>,
    /// Interner for creating new symbols (e.g., _rn for ROW_NUMBER).
    pub(super) interner: yelang_interner::Interner,
}

impl UnnestingState {
    pub(super) fn new(annotations: AccessingAnnotation, interner: yelang_interner::Interner) -> Self {
        Self {
            infos: Vec::new(),
            stack: Vec::new(),
            annotations,
            cache: FxHashMap::default(),
            interner,
        }
    }

    /// Push a new Unnesting frame onto the stack.
    pub(super) fn push(&mut self, unnesting: Unnesting) -> usize {
        let idx = self.stack.len();
        self.stack.push(unnesting);
        idx
    }

    /// Pop the top Unnesting frame.
    pub(super) fn pop(&mut self) {
        self.stack.pop();
    }

    /// Get the current (top) Unnesting frame.
    pub(super) fn current(&self) -> Option<&Unnesting> {
        self.stack.last()
    }

    /// Get the current (top) Unnesting frame mutably.
    pub(super) fn current_mut(&mut self) -> Option<&mut Unnesting> {
        self.stack.last_mut()
    }

    /// Get the parent Unnesting frame (if any).
    pub(super) fn parent(&self) -> Option<&Unnesting> {
        if self.stack.len() >= 2 {
            Some(&self.stack[self.stack.len() - 2])
        } else {
            None
        }
    }

    /// Create a new UnnestingInfo and return its index.
    pub(super) fn alloc_info(&mut self, info: UnnestingInfo) -> usize {
        let idx = self.infos.len();
        self.infos.push(info);
        idx
    }

    /// BTW 2025: merge parent's outer_refs into the current unnesting.
    /// Implements "never push different D sets across dependent joins."
    pub(super) fn merge_parent_outer_refs(&mut self) {
        let len = self.stack.len();
        if len < 2 {
            return;
        }
        let parent_info_idx = self.stack[len - 2].info_idx;
        let parent_refs: Vec<Symbol> = self.infos[parent_info_idx].outer_refs.clone();
        let current_info_idx = self.stack[len - 1].info_idx;
        for r in parent_refs {
            if !self.infos[current_info_idx].outer_refs.contains(&r) {
                self.infos[current_info_idx].outer_refs.push(r);
            }
        }
    }
}
