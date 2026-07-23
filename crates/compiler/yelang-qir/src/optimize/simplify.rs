//! Simplification rules — remove trivially redundant nodes.

use crate::optimize::{ApplyOrder, OptRule};
use crate::plan::{Plan, PlanArena, PlanId};
use crate::tree::Transformed;

// ---------------------------------------------------------------------------
// EliminateTrivialFilter
// ---------------------------------------------------------------------------

/// Remove `Filter { pred: true }` — a filter that always passes.
///
/// Detects literal `true` predicates by inspecting the HIR expression.
/// For now, this is a placeholder: full detection requires access to
/// the HIR expression arena to check for `Lit::Bool(true)`.
pub struct EliminateTrivialFilter;

impl OptRule for EliminateTrivialFilter {
    fn name(&self) -> &str {
        "eliminate_trivial_filter"
    }

    fn apply_order(&self) -> ApplyOrder {
        ApplyOrder::BottomUp
    }

    fn rewrite(&self, id: PlanId, arena: &mut PlanArena) -> Transformed {
        // TODO: inspect the predicate expression to detect literal `true`.
        // For now, this rule is a no-op placeholder.
        let _ = (id, arena);
        Transformed::no(id)
    }
}

// ---------------------------------------------------------------------------
// EliminateTrivialLimit
// ---------------------------------------------------------------------------

/// Remove `Limit { skip: None, fetch: None }` — a limit that does nothing.
pub struct EliminateTrivialLimit;

impl OptRule for EliminateTrivialLimit {
    fn name(&self) -> &str {
        "eliminate_trivial_limit"
    }

    fn apply_order(&self) -> ApplyOrder {
        ApplyOrder::BottomUp
    }

    fn rewrite(&self, id: PlanId, arena: &mut PlanArena) -> Transformed {
        let plan = arena.plan(id);
        if let Plan::Limit {
            input,
            skip: None,
            fetch: None,
        } = plan
        {
            return Transformed::yes(*input);
        }
        Transformed::no(id)
    }
}

// ---------------------------------------------------------------------------
// MergeAdjacentFilters
// ---------------------------------------------------------------------------

/// Merge `Filter(p1, Filter(p2, input))` → `Filter(p1 AND p2, input)`.
///
/// Reduces the number of filter nodes and enables further pushdown.
/// For now, this is a placeholder: full merging requires constructing
/// a binary AND expression in the HIR arena.
pub struct MergeAdjacentFilters;

impl OptRule for MergeAdjacentFilters {
    fn name(&self) -> &str {
        "merge_adjacent_filters"
    }

    fn apply_order(&self) -> ApplyOrder {
        ApplyOrder::BottomUp
    }

    fn rewrite(&self, id: PlanId, arena: &mut PlanArena) -> Transformed {
        // TODO: detect Filter → Filter chains and merge predicates
        // by constructing a Binary { op: And, left: p1, right: p2 }
        // expression in the HIR arena.
        let _ = (id, arena);
        Transformed::no(id)
    }
}
