//! Unnesting state for dependent join elimination.

use yelang_arena::FxHashMap;
use yelang_interner::Symbol;

use crate::plan::PlanId;

use super::union_find::UnionFind;

/// State for one `DependentJoin` being eliminated.
///
/// Created when we encounter a `DependentJoin` during the top-down
/// traversal. Carries the domain projection, column equivalences, and
/// the substitution map.
#[derive(Debug)]
#[allow(dead_code)] // Fields used in full BTW 2025 implementation
pub(super) struct UnnestingInfo {
    /// The `DependentJoin` node being eliminated.
    pub(super) join_id: PlanId,
    /// Outer attributes referenced by the inner side: A(outer) ∩ F(inner).
    pub(super) outer_refs: Vec<Symbol>,
    /// Union-find of equivalent columns (from join predicates).
    pub(super) cclasses: UnionFind,
    /// Map from outer-ref columns to their substitutes after union-find.
    /// If `repr[c] = d`, then `c` can be replaced by `d` in the inner plan.
    pub(super) repr: FxHashMap<Symbol, Symbol>,
    /// Parent unnesting (for nested dependent joins).
    pub(super) parent: Option<usize>,
}

/// Stack of active unnesting states.
pub(super) struct UnnestingState {
    pub(super) stack: Vec<UnnestingInfo>,
    /// CTE DAG cutting: maps original PlanId → decorrelated PlanId.
    /// Ensures shared subtrees (CTEs referenced multiple times) are
    /// processed only once (BTW 2025 §4.3).
    pub(super) cache: FxHashMap<PlanId, PlanId>,
}

impl UnnestingState {
    pub(super) fn new() -> Self {
        Self {
            stack: Vec::new(),
            cache: FxHashMap::default(),
        }
    }

    pub(super) fn push(&mut self, info: UnnestingInfo) -> usize {
        let idx = self.stack.len();
        self.stack.push(info);
        idx
    }

    pub(super) fn pop(&mut self) {
        self.stack.pop();
    }

    pub(super) fn current(&self) -> Option<&UnnestingInfo> {
        self.stack.last()
    }

    #[allow(dead_code)]
    pub(super) fn current_mut(&mut self) -> Option<&mut UnnestingInfo> {
        self.stack.last_mut()
    }

    /// BTW 2025 §4.3: merge parent's outer_refs into the current unnesting.
    /// This implements "never push different D sets across dependent joins."
    pub(super) fn merge_parent_outer_refs(&mut self) {
        let len = self.stack.len();
        if len < 2 {
            return;
        }
        // Collect parent's outer_refs, then extend current.
        let parent_refs: Vec<Symbol> = self.stack[len - 2].outer_refs.clone();
        let current = &mut self.stack[len - 1];
        for r in parent_refs {
            if !current.outer_refs.contains(&r) {
                current.outer_refs.push(r);
            }
        }
    }
}
