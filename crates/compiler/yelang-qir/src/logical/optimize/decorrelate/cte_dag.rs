//! CTE DAG cutting (BTW 2025 §4.3).
//!
//! When the algebra is a DAG (CTE referenced multiple times), the
//! standard tree-walking unnesting would process shared subtrees
//! multiple times. DAG cutting prevents this:
//!
//! 1. Detect shared operators (referenced by multiple parents).
//! 2. Cut the DAG at shared operators — they form their own subtrees.
//! 3. Shared reads become accessing operators.
//! 4. One designated read (smallest PlanId) triggers unnesting of
//!    the shared subtree.
//!
//! The `UnnestingState.cache` maps original PlanId → decorrelated PlanId,
//! ensuring each shared subtree is unnested exactly once.

use crate::logical::plan::{PlanArena, PlanId};

use super::state::UnnestingState;

/// Detect shared operators in the plan tree.
///
/// Returns a set of PlanIds that are referenced by multiple parents
/// (i.e., they appear more than once in the tree).
pub(super) fn detect_shared_operators(
    root: PlanId,
    arena: &PlanArena,
) -> yelang_arena::FxHashSet<PlanId> {
    use yelang_arena::FxHashMap;

    let mut ref_count: FxHashMap<PlanId, usize> = FxHashMap::default();
    count_references(root, arena, &mut ref_count);

    ref_count
        .iter()
        .filter(|&(_, &count)| count > 1)
        .map(|(&id, _)| id)
        .collect()
}

/// Count how many times each node is referenced in the tree.
fn count_references(
    node: PlanId,
    arena: &PlanArena,
    counts: &mut yelang_arena::FxHashMap<PlanId, usize>,
) {
    *counts.entry(node).or_insert(0) += 1;
    for child in crate::tree::children(arena.plan(node)) {
        count_references(child, arena, counts);
    }
}

/// Check if a node has already been decorrelated (CTE cache hit).
pub(super) fn is_cached(node: PlanId, state: &UnnestingState) -> Option<PlanId> {
    state.cache.get(&node).copied()
}

/// Cache a decorrelated result.
pub(super) fn cache_result(node: PlanId, result: PlanId, state: &mut UnnestingState) {
    state.cache.insert(node, result);
}
