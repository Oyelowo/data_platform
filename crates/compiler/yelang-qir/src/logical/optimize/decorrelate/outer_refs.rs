//! Compute outer references for dependent join elimination.

use yelang_interner::Symbol;

use crate::analysis::referenced_fields;
use crate::logical::plan::{ExprRef, PlanArena, PlanId};

/// Compute the outer references: symbols produced by `outer` that are
/// referenced by `inner` or the join predicate.
pub(super) fn compute_outer_refs(
    outer: PlanId,
    inner: PlanId,
    pred: Option<ExprRef>,
    arena: &PlanArena,
) -> Vec<Symbol> {
    use crate::analysis::plan_output_fields;

    let outer_fields = if let Some(outer_plan) = arena.get(outer) {
        plan_output_fields(outer_plan, arena)
    } else {
        return vec![];
    };

    // Collect fields referenced by the inner plan and predicate.
    let mut inner_refs = yelang_arena::FxHashSet::new();

    if let Some(pred_expr) = pred {
        for f in referenced_fields(pred_expr, arena).iter() {
            inner_refs.insert(*f);
        }
    }

    // Walk the inner plan tree to collect all referenced fields.
    collect_plan_refs(inner, arena, &mut inner_refs);

    // Intersection: fields that are both produced by outer and referenced by inner.
    outer_fields
        .iter()
        .filter(|f| inner_refs.contains(f))
        .copied()
        .collect()
}

/// Recursively collect all field references from a plan subtree.
fn collect_plan_refs(
    node: PlanId,
    arena: &PlanArena,
    out: &mut yelang_arena::FxHashSet<Symbol>,
) {
    let Some(plan) = arena.get(node) else {
        return;
    };

    let plan_refs = crate::analysis::plan_referenced_fields(plan, arena);
    for f in plan_refs.iter() {
        out.insert(*f);
    }

    // Recurse into children.
    for child in crate::tree::children(plan) {
        collect_plan_refs(child, arena, out);
    }
}
