//! Simplification rules — remove trivially redundant nodes.

use crate::optimize::{ApplyOrder, OptRule};
use crate::logical::plan::{Plan, PlanArena, PlanId};
use crate::tree::Transformed;

/// Remove `Filter { pred: true }` — a filter that always passes.
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
        let _ = (id, arena);
        Transformed::no(id)
    }
}

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

/// Merge `Filter(p1, Filter(p2, input))` → `Filter(p1 AND p2, input)`.
pub struct MergeAdjacentFilters;

impl OptRule for MergeAdjacentFilters {
    fn name(&self) -> &str {
        "merge_adjacent_filters"
    }

    fn apply_order(&self) -> ApplyOrder {
        ApplyOrder::BottomUp
    }

    fn rewrite(&self, id: PlanId, arena: &mut PlanArena) -> Transformed {
        // TODO: detect Filter → Filter chains and merge predicates.
        let _ = (id, arena);
        Transformed::no(id)
    }
}
