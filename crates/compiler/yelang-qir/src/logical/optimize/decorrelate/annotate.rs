//! Phase 1: Identify non-trivial dependent joins (BTW 2025 §3.1).
//!
//! For every column reference in the plan tree, compute the LCA
//! (lowest common ancestor) of the accessor and provider. If the LCA
//! is a DependentJoin, annotate it with the accessing operator.
//!
//! DependentJoins with empty accessing sets are trivial → convert
//! directly to regular joins.

use yelang_arena::FxHashMap;
use yelang_interner::Symbol;

use crate::logical::plan::{Plan, PlanArena, PlanId};

use super::state::AccessingAnnotation;

/// Annotate all dependent joins with their accessing operators.
///
/// Walks the plan tree and for each column reference, determines
/// which dependent join (if any) separates the accessor from the
/// provider. The accessor is then added to that dependent join's
/// accessing set.
///
/// Simplified implementation: instead of full LCA computation with
/// indexed algebra (O(log n) per access), we walk the tree top-down
/// and track which dependent joins are "open" (between the current
/// node and the root). For each column reference, we check if any
/// open dependent join's outer side provides the column.
pub(super) fn annotate_accessing(
    root: PlanId,
    arena: &PlanArena,
) -> AccessingAnnotation {
    let mut annotations = AccessingAnnotation::new();

    // Collect all dependent join nodes.
    let mut dependent_joins: Vec<PlanId> = Vec::new();
    collect_dependent_joins(root, arena, &mut dependent_joins);

    if dependent_joins.is_empty() {
        return annotations;
    }

    // For each dependent join, find its outer side's output fields.
    let mut outer_fields: FxHashMap<PlanId, Vec<Symbol>> = FxHashMap::default();
    for &dj_id in &dependent_joins {
        if let Plan::DependentJoin { outer, .. } = arena.plan(dj_id) {
            let fields = crate::analysis::plan_output_fields(arena.plan(*outer), arena);
            outer_fields.insert(dj_id, fields.iter().copied().collect());
        }
    }

    // Walk the inner side of each dependent join and find operators
    // that reference outer columns.
    for &dj_id in &dependent_joins {
        if let Plan::DependentJoin { inner, .. } = arena.plan(dj_id) {
            let outer_syms: Vec<Symbol> = outer_fields.get(&dj_id).cloned().unwrap_or_default();
            find_accessing_operators(*inner, &outer_syms, dj_id, arena, &mut annotations);
        }
    }

    annotations
}

/// Collect all DependentJoin node IDs in the plan tree.
fn collect_dependent_joins(
    node: PlanId,
    arena: &PlanArena,
    out: &mut Vec<PlanId>,
) {
    if let Plan::DependentJoin { .. } = arena.plan(node) {
        out.push(node);
    }
    for child in crate::tree::children(arena.plan(node)) {
        collect_dependent_joins(child, arena, out);
    }
}

/// Find operators in the inner subtree that reference outer columns.
///
/// For each operator, check if its expressions reference any of the
/// outer columns. If so, add it to the accessing set.
fn find_accessing_operators(
    node: PlanId,
    outer_syms: &[Symbol],
    dj_id: PlanId,
    arena: &PlanArena,
    annotations: &mut AccessingAnnotation,
) {
    if outer_syms.is_empty() {
        return;
    }

    // Check if this operator references any outer columns.
    let refs = crate::analysis::plan_referenced_fields(arena.plan(node), arena);
    let accesses_outer = refs.iter().any(|r| outer_syms.contains(r));

    if accesses_outer {
        annotations.add(dj_id, node);
    }

    // Recurse into children.
    for child in crate::tree::children(arena.plan(node)) {
        find_accessing_operators(child, outer_syms, dj_id, arena, annotations);
    }
}
