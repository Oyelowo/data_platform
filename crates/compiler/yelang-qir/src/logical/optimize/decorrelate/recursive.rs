//! WITH RECURSIVE unnesting (BTW 2025 §4.2).
//!
//! Recursive CTEs are translated to `iterate` + `iteratescan` operators.
//! During unnesting:
//!
//! 1. Add outer refs to CTE columns (seed + iteration sides).
//! 2. Mark all `iteratescan` operators as accessing operators.
//! 3. Unnest seed side first (makes new columns available).
//! 4. Then unnest iteration side.
//!
//! Current status: The Plan enum uses `Repeat` for recursive operations,
//! which doesn't match the BTW 2025 iterate/iteratescan model. Full
//! implementation requires adding `Iterate` and `IterateScan` variants
//! to the Plan enum.
//!
//! TODO: Add Iterate/IterateScan Plan variants and implement the full
//! WITH RECURSIVE unnesting algorithm.

use crate::logical::plan::{PlanArena, PlanId};

use super::state::UnnestingState;

/// Unnest a Repeat (recursive CTE) node.
///
/// Currently a pass-through — the Repeat node is left unchanged.
/// Full implementation requires Iterate/IterateScan Plan variants.
pub(super) fn unnest_repeat(
    node: PlanId,
    _state: &mut UnnestingState,
    _arena: &mut PlanArena,
) -> PlanId {
    // TODO: Implement WITH RECURSIVE unnesting when Iterate/IterateScan
    // variants are added to the Plan enum.
    //
    // The algorithm:
    // 1. Add outer refs to CTE columns
    // 2. Mark iteratescans as accessing
    // 3. Unnest seed side first
    // 4. Unnest iteration side
    node
}
