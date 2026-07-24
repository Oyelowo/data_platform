//! Predicate equivalence collection via union-find (BTW 2025 §3.2).
//!
//! Extracts column equivalences (a = b) from predicates and adds them
//! to the union-find. These equivalences drive the substitution decision:
//! if an outer ref has an equivalence with an inner column, we can
//! substitute instead of building a domain join.

use yelang_interner::Symbol;
use yelang_thir::ThirExpr;

use crate::logical::plan::{ExprRef, PlanArena};

use super::state::Unnesting;
use super::union_find::UnionFind;

/// Add column equivalences from a predicate expression to the unnesting's union-find.
///
/// Walks the THIR expression tree looking for equality comparisons between
/// field accesses: `a.x == b.y` → `union(x, y)`.
/// Also recurses through `And` conjunctions.
pub(super) fn add_predicate_equivalences(
    pred: ExprRef,
    arena: &PlanArena,
    unnesting: &mut Unnesting,
) {
    collect_equivalences(pred, arena, &mut unnesting.cclasses);
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

/// Populate the repr map from cclasses for the given outer refs.
///
/// For each outer ref, if it has an equivalence with a non-outer column,
/// add it to repr. This enables substitution instead of domain join.
pub(super) fn populate_repr(
    unnesting: &mut Unnesting,
    outer_refs: &[Symbol],
) {
    let outer_set: yelang_arena::FxHashSet<Symbol> = outer_refs.iter().copied().collect();

    for &outer_ref in outer_refs {
        // Find the representative of the outer ref's equivalence class.
        let repr_sym = unnesting.cclasses.find(outer_ref);
        if repr_sym != outer_ref && !outer_set.contains(&repr_sym) {
            // The representative is a non-outer column → substitution possible.
            unnesting.repr.insert(outer_ref, repr_sym);
        }
    }
}
