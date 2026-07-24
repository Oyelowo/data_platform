//! WITH RECURSIVE unnesting (BTW 2025 §4.2).
//!
//! Recursive CTEs use `Iterate` + `IterateScan` operators.
//! During unnesting:
//!
//! 1. Add outer refs to CTE columns (seed + iteration sides).
//! 2. Mark all `IterateScan` operators as accessing operators.
//! 3. Unnest seed side first (makes new columns available).
//! 4. Then unnest iteration side.
//!
//! The `Iterate` node is treated as an opaque barrier for now —
//! full unnesting of recursive CTEs requires the iteration to
//! converge before the outer correlation can be resolved.

use crate::logical::plan::{Plan, PlanArena, PlanId};

use super::state::UnnestingState;

/// Unnest an Iterate (recursive CTE) node.
///
/// BTW 2025 §4.2:
/// 1. Add outer refs to CTE columns
/// 2. Mark IterateScan operators as accessing
/// 3. Unnest seed side first
/// 4. Unnest iteration side
///
/// For now, we recurse into both sides but treat the Iterate as a
/// barrier — the recursive correlation is not fully unnested.
pub(super) fn unnest_iterate(
    node: PlanId,
    state: &mut UnnestingState,
    arena: &mut PlanArena,
) -> PlanId {
    let (seed, iteration, cte_name, max_iters) = match arena.plan(node) {
        Plan::Iterate { seed, iteration, cte_name, max_iters } => {
            (*seed, *iteration, *cte_name, *max_iters)
        }
        _ => return node,
    };

    // Unnest seed side first (makes new columns available).
    let new_seed = super::eliminate::eliminate_top_down(seed, state, arena);

    // Unnest iteration side.
    let new_iteration = super::eliminate::eliminate_top_down(iteration, state, arena);

    // Rebuild the Iterate node with unnested children.
    arena.alloc(Plan::Iterate {
        seed: new_seed,
        iteration: new_iteration,
        cte_name,
        max_iters,
    })
}
