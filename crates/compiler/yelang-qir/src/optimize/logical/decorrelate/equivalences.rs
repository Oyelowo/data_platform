//! Predicate equivalence collection via union-find.

use yelang_thir::ThirExpr;

use crate::plan::{ExprRef, PlanArena};

use super::state::UnnestingState;
use super::union_find::UnionFind;

/// Add column equivalences from a predicate expression to the union-find.
///
/// Walks the THIR expression tree looking for equality comparisons between
/// field accesses: `a.x == b.y` → `union(x, y)`.
/// Also recurses through `And` conjunctions.
pub(super) fn add_predicate_equivalences(
    pred: ExprRef,
    arena: &PlanArena,
    state: &mut UnnestingState,
) {
    let Some(info) = state.current_mut() else {
        return;
    };
    collect_equivalences(pred, arena, &mut info.cclasses);
}

/// Recursively collect field equivalences from a THIR expression.
fn collect_equivalences(
    expr: ExprRef,
    arena: &PlanArena,
    uf: &mut UnionFind,
) {
    let Some(expr_node) = arena.thir_expr(expr) else {
        return;
    };

    match expr_node {
        // a.x == b.y → union(x, y)
        ThirExpr::Binary {
            op: yelang_ast::BinaryOp::Eq,
            left,
            right,
        } => {
            if let (
                Some(ThirExpr::Field { field: left_field, .. }),
                Some(ThirExpr::Field { field: right_field, .. }),
            ) = (arena.thir_expr(*left), arena.thir_expr(*right))
            {
                uf.union(*left_field, *right_field);
            }
            // Also recurse into both sides for nested equalities.
            collect_equivalences(*left, arena, uf);
            collect_equivalences(*right, arena, uf);
        }

        // Recurse through AND conjunctions.
        ThirExpr::Binary {
            op: yelang_ast::BinaryOp::And,
            left,
            right,
        } => {
            collect_equivalences(*left, arena, uf);
            collect_equivalences(*right, arena, uf);
        }

        // Recurse into other binary ops (may contain nested equalities).
        ThirExpr::Binary { left, right, .. } => {
            collect_equivalences(*left, arena, uf);
            collect_equivalences(*right, arena, uf);
        }

        // Recurse into unary, calls, etc.
        ThirExpr::Unary { expr: inner, .. } => {
            collect_equivalences(*inner, arena, uf);
        }
        ThirExpr::Call { args, .. } => {
            for &arg in args {
                collect_equivalences(arg, arena, uf);
            }
        }
        ThirExpr::Intrinsic { args, .. } => {
            for &arg in args {
                collect_equivalences(arg, arena, uf);
            }
        }

        _ => {}
    }
}
