//! Top-down dependent join elimination driver (BTW 2025 §3.2).
//!
//! Walks the plan tree top-to-bottom. When a DependentJoin is found,
//! calls `eliminate_dependent_join`. Nested DependentJoins are handled
//! recursively within the parent's unnesting context.

use crate::logical::plan::{Plan, PlanArena, PlanId};

use super::dependent_join::eliminate_dependent_join;
use super::state::UnnestingState;

/// Top-down elimination of all dependent joins in the plan tree.
///
/// This is the main Phase 2 driver. It walks the tree from root to leaves.
/// When it encounters a DependentJoin, it eliminates it. Nested DependentJoins
/// are handled by the per-operator rules (which call eliminate_dependent_join
/// recursively).
pub(super) fn eliminate_top_down(
    node: PlanId,
    state: &mut UnnestingState,
    arena: &mut PlanArena,
) -> PlanId {
    // Check the CTE cache first.
    if let Some(&cached) = state.cache.get(&node) {
        return cached;
    }

    let result = match arena.plan(node).clone() {
        Plan::DependentJoin { outer, inner, pred, kind } => {
            eliminate_dependent_join(node, outer, inner, pred, kind, state, arena)
        }

        // For all other operators, recurse into children top-down.
        _ => {
            let child_ids = crate::tree::children(arena.plan(node));
            if child_ids.is_empty() {
                node
            } else {
                // Recursively eliminate dependent joins in children.
                let new_children: Vec<PlanId> = child_ids
                    .iter()
                    .map(|&child| eliminate_top_down(child, state, arena))
                    .collect();

                // Rebuild the node with decorrelated children.
                let new_plan = crate::tree::map_children(arena.plan(node), &new_children);
                arena.alloc(new_plan)
            }
        }
    };

    // Cache the result for CTE DAG cutting.
    state.cache.insert(node, result);
    result
}
