//! Column rewriting helpers (BTW 2025 Fig 5).
//!
//! After unnesting determines substitutions (outer_ref → local column),
//! these functions rewrite all column references in plan expressions.
//!
//! Column references in THIR are `Field { base, field }` where `field`
//! is a Symbol (column name). Rewriting replaces `field` with the
//! substitute from the repr map.

use yelang_interner::Symbol;

use crate::logical::plan::{ExprRef, PlanArena};

use super::state::Unnesting;
use super::state::UnnestingState;

/// Rewrite all column references in an expression using the unnesting's
/// substitution map (`repr`).
pub(super) fn rewrite_expr(
    expr: &mut ExprRef,
    unnesting: &Unnesting,
    _state: &UnnestingState,
    arena: &mut PlanArena,
) {
    if unnesting.repr.is_empty() {
        return;
    }
    let thir_id = *expr;
    rewrite_thir_expr(thir_id, &unnesting.repr, arena);
}

/// Rewrite column references in a THIR expression tree.
///
/// Walks the expression tree and replaces `Field { base, field }` nodes
/// where `field` is in the repr map. The `field` Symbol is replaced with
/// the substitute; the `base` (row variable) stays the same.
fn rewrite_thir_expr(
    expr_id: yelang_thir::ids::ThirExprId,
    repr: &yelang_arena::FxHashMap<Symbol, Symbol>,
    arena: &mut PlanArena,
) {
    use yelang_thir::ThirExpr;

    let expr = arena.thir_expr(expr_id).cloned();
    if let Some(expr) = expr {
        match &expr {
            // Column reference: Field { base, field }
            // Replace `field` if it's in repr.
            ThirExpr::Field { base, field } => {
                if let Some(&new_field) = repr.get(field) {
                    let new_expr = ThirExpr::Field {
                        base: *base,
                        field: new_field,
                    };
                    arena.set_thir_expr(expr_id, new_expr);
                    return;
                }
                // Recurse into base.
                rewrite_thir_expr(*base, repr, arena);
            }

            // Binary: recurse into both sides.
            ThirExpr::Binary { left, right, .. } => {
                rewrite_thir_expr(*left, repr, arena);
                rewrite_thir_expr(*right, repr, arena);
            }

            // Unary: recurse into operand.
            ThirExpr::Unary { expr: inner, .. } => {
                rewrite_thir_expr(*inner, repr, arena);
            }

            // Call: recurse into func and args.
            ThirExpr::Call { func, args } => {
                rewrite_thir_expr(*func, repr, arena);
                for &arg in args {
                    rewrite_thir_expr(arg, repr, arena);
                }
            }

            // Block: recurse into stmts and tail.
            ThirExpr::Block { stmts: _, tail } => {
                // stmts are ThirStmtId — we'd need to walk into them.
                // For now, only recurse into tail.
                if let Some(tail_expr) = tail {
                    rewrite_thir_expr(*tail_expr, repr, arena);
                }
            }

            // If: recurse into cond (branches are ThirBodyId — skip for now).
            ThirExpr::If { cond, .. } => {
                rewrite_thir_expr(*cond, repr, arena);
            }

            // Struct: recurse into field expressions.
            ThirExpr::Struct { fields, rest, .. } => {
                for &(_, field_expr) in fields {
                    rewrite_thir_expr(field_expr, repr, arena);
                }
                if let Some(rest_expr) = rest {
                    rewrite_thir_expr(*rest_expr, repr, arena);
                }
            }

            // Object: recurse into field expressions.
            ThirExpr::Object { fields } => {
                for &(_, field_expr) in fields {
                    rewrite_thir_expr(field_expr, repr, arena);
                }
            }

            // Tuple: recurse into fields.
            ThirExpr::Tuple { fields } => {
                for &field in fields {
                    rewrite_thir_expr(field, repr, arena);
                }
            }

            // Array: recurse into exprs.
            ThirExpr::Array { exprs } => {
                for &e in exprs {
                    rewrite_thir_expr(e, repr, arena);
                }
            }

            // Index: recurse into base and index.
            ThirExpr::Index { base, index } => {
                rewrite_thir_expr(*base, repr, arena);
                rewrite_thir_expr(*index, repr, arena);
            }

            // Cast: recurse into inner expression.
            ThirExpr::Cast { expr: inner, .. } => {
                rewrite_thir_expr(*inner, repr, arena);
            }

            // Assign: recurse into both sides.
            ThirExpr::Assign { left, right } => {
                rewrite_thir_expr(*left, repr, arena);
                rewrite_thir_expr(*right, repr, arena);
            }

            // Range: recurse into start and end.
            ThirExpr::Range { start, end, .. } => {
                if let Some(s) = start {
                    rewrite_thir_expr(*s, repr, arena);
                }
                if let Some(e) = end {
                    rewrite_thir_expr(*e, repr, arena);
                }
            }

            // Match: recurse into scrutinee.
            ThirExpr::Match { scrutinee, .. } => {
                rewrite_thir_expr(*scrutinee, repr, arena);
            }

            // Closure: don't rewrite inside — closures have their own scope.
            ThirExpr::Closure { .. } => {}

            // Query: don't rewrite inside — nested queries have their own context.
            ThirExpr::Query(_) => {}

            // Leaf nodes: no column references to rewrite.
            ThirExpr::Literal(_)
            | ThirExpr::Var(_)
            | ThirExpr::Local(_)
            | ThirExpr::Loop { .. }
            | ThirExpr::Break { .. }
            | ThirExpr::Continue { .. }
            | ThirExpr::Return { .. } => {}

            _ => {}
        }
    }
}

/// Rewrite column references in a join condition, taking into account
/// which side of the join provides the replacement.
pub(super) fn rewrite_expr_for_join(
    expr: &mut ExprRef,
    left_unnest: &Unnesting,
    right_unnest: &Unnesting,
    _state: &UnnestingState,
    arena: &mut PlanArena,
) {
    let mut merged = left_unnest.repr.clone();
    for (k, v) in &right_unnest.repr {
        merged.entry(*k).or_insert(*v);
    }
    if merged.is_empty() {
        return;
    }
    let thir_id = *expr;
    rewrite_thir_expr(thir_id, &merged, arena);
}

/// Check if a symbol is in the unnesting's outer refs.
pub(super) fn is_outer_ref(sym: Symbol, unnesting: &Unnesting, state: &UnnestingState) -> bool {
    unnesting.info(state).outer_refs.contains(&sym)
}

/// Get the substitution for an outer ref, if any.
pub(super) fn get_repr(sym: Symbol, unnesting: &Unnesting) -> Option<Symbol> {
    unnesting.repr.get(&sym).copied()
}
