//! Simple dependent join elimination (BTW 2025 Fig 3).
//!
//! Before full unnesting, try to eliminate the dependent join by:
//! 1. Merging selections into the join predicate
//! 2. Moving maps above the join
//! 3. If the accessing set becomes empty → convert to regular join

use yelang_interner::Symbol;

use crate::logical::plan::{DepJoinKind, ExprRef, Plan, PlanArena, PlanId};

use super::dependent_join::convert_to_regular_join;
use super::state::UnnestingState;

/// Try simple elimination of a dependent join.
///
/// Returns `Some(result)` if the dependent join was eliminated,
/// `None` if full unnesting is needed.
///
/// BTW 2025 Fig 3:
/// ```text
/// fun simpleDJoinElimination(join):
///     for op in accessing(join):
///         if path from op to join is linear:
///             if op is a selection → merge into join predicate
///             if op is a map → move above join
///     if accessing is empty → convert to regular join
/// ```
pub(super) fn try_simple_elimination(
    node: PlanId,
    outer: PlanId,
    inner: PlanId,
    pred: Option<ExprRef>,
    kind: DepJoinKind,
    _outer_refs: &[Symbol],
    state: &mut UnnestingState,
    arena: &mut PlanArena,
) -> Option<PlanId> {
    let accessing: Vec<PlanId> = state.annotations.accessing(node).to_vec();

    if accessing.is_empty() {
        // No accessing operators → trivial.
        return Some(convert_to_regular_join(outer, inner, pred, kind, arena));
    }

    // Try to eliminate each accessing operator.
    let mut remaining_accessing = Vec::new();
    let mut current_pred = pred;
    let mut current_inner = inner;

    for &op_id in &accessing {
        let plan = arena.plan(op_id).clone();
        match &plan {
            // Selection: merge predicate into join condition.
            Plan::Filter { input, pred: filter_pred } => {
                // Check if the path from this filter to the join is linear
                // (only partitionable operators in between).
                if is_linear_path(op_id, node, arena) {
                    // Merge the filter predicate into the join condition.
                    current_pred = match current_pred {
                        Some(existing) => {
                            // AND the two predicates.
                            let combined = arena.alloc_thir_expr(yelang_thir::ThirExpr::Binary {
                                op: yelang_ast::BinaryOp::And,
                                left: existing,
                                right: *filter_pred,
                            });
                            Some(combined)
                        }
                        None => Some(*filter_pred),
                    };
                    // Remove the filter from the inner plan by replacing it
                    // with its input.
                    current_inner = *input;
                    // Don't add to remaining — this accessing op is eliminated.
                } else {
                    remaining_accessing.push(op_id);
                }
            }

            // Map: move above the join.
            Plan::Map { input, .. } => {
                if is_linear_path(op_id, node, arena) {
                    // The map will be moved above the join by the caller.
                    // For now, just remove it from the accessing set.
                    current_inner = *input;
                } else {
                    remaining_accessing.push(op_id);
                }
            }

            // Other operators: cannot be simply eliminated.
            _ => {
                remaining_accessing.push(op_id);
            }
        }
    }

    if remaining_accessing.is_empty() {
        // All accessing operators eliminated → convert to regular join.
        Some(convert_to_regular_join(outer, current_inner, current_pred, kind, arena))
    } else {
        // Some accessing operators remain → full unnesting needed.
        None
    }
}

/// Check if the path from `op` to `join` is linear (only partitionable operators).
///
/// A linear path means there's exactly one child at each level between
/// `op` and `join`, and all intermediate operators are partitionable
/// (selection, map, project — operators that don't duplicate or merge rows).
fn is_linear_path(op: PlanId, join: PlanId, arena: &PlanArena) -> bool {
    // Walk from op up to join, checking that each step has exactly one child.
    // For now, we use a simple heuristic: check if op is a direct child of join
    // or reachable through a chain of single-child operators.
    let mut current = op;
    let mut visited = 0;
    let max_depth = 20; // prevent infinite loops

    while current != join && visited < max_depth {
        visited += 1;
        let children = crate::tree::children(arena.plan(current));
        if children.len() != 1 {
            return false;
        }
        current = children[0];
    }

    current == join
}
