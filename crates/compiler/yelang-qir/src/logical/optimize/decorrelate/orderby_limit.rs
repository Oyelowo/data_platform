//! ORDER BY LIMIT rewrite (BTW 2025 §4.4).
//!
//! Rewrites `ORDER BY x LIMIT l OFFSET o` subqueries into window queries:
//!
//! ```sql
//! -- Before:
//! SELECT * FROM S ORDER BY x LIMIT l OFFSET o
//!
//! -- After:
//! SELECT * FROM (
//!     SELECT *, ROW_NUMBER() OVER (ORDER BY x) AS RN FROM S
//! ) S WHERE RN BETWEEN o+1 AND l+o
//! ```
//!
//! During unnesting, outer refs are added as PARTITION BY entries to the
//! OVER clause, making the limit per-outer-binding instead of global.

use crate::logical::plan::{Plan, PlanArena, PlanId};

use super::state::UnnestingState;

/// Detect and rewrite an ORDER BY + LIMIT pattern under a dependent join.
///
/// Returns `Some(new_plan)` if the pattern was detected and rewritten,
/// `None` if the pattern doesn't match.
///
/// Pattern: Sort { input, specs } → Limit { input: Sort, skip, fetch }
/// under a DependentJoin's inner side.
///
/// TODO: This requires interner access to create the `_rn` column symbol.
/// Currently returns None (no rewrite) until the interner is threaded
/// through the decorrelation code.
pub(super) fn try_rewrite_orderby_limit(
    _sort_node: PlanId,
    _limit_node: PlanId,
    _state: &mut UnnestingState,
    _arena: &mut PlanArena,
) -> Option<PlanId> {
    // TODO: Implement when interner is available in decorrelation context.
    // The rewrite creates:
    //   Window { input: sort_input, funcs: [ROW_NUMBER() OVER (ORDER BY specs PARTITION BY outer_refs)] }
    //   → Filter { input: window, pred: _rn BETWEEN skip+1 AND skip+fetch }
    None
}

/// Check if a plan subtree matches the ORDER BY + LIMIT pattern.
///
/// Returns (sort_node, limit_node) if the pattern matches.
pub(super) fn detect_orderby_limit_pattern(
    node: PlanId,
    arena: &PlanArena,
) -> Option<(PlanId, PlanId)> {
    match arena.plan(node) {
        Plan::Limit { input, .. } => {
            match arena.plan(*input) {
                Plan::Sort { .. } => Some((*input, node)),
                _ => None,
            }
        }
        _ => None,
    }
}
