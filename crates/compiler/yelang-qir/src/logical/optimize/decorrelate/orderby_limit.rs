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

use yelang_interner::Symbol;

use crate::logical::plan::{
    Plan, PlanArena, PlanId, SortSpec, WindowFunc, WindowKind,
};

use super::state::UnnestingState;

/// Detect and rewrite an ORDER BY + LIMIT pattern under a dependent join.
///
/// Returns `Some(new_plan)` if the pattern was detected and rewritten,
/// `None` if the pattern doesn't match.
///
/// Pattern: Limit { input: Sort { input, specs }, skip, fetch }
pub(super) fn try_rewrite_orderby_limit(
    limit_node: PlanId,
    state: &mut UnnestingState,
    arena: &mut PlanArena,
) -> Option<PlanId> {
    // Verify the pattern: Limit → Sort.
    let (sort_input, sort_specs, skip, fetch) = match arena.plan(limit_node) {
        Plan::Limit { input, skip, fetch } => {
            match arena.plan(*input) {
                Plan::Sort { input: sort_input, specs } => {
                    (*sort_input, specs.clone(), *skip, *fetch)
                }
                _ => return None,
            }
        }
        _ => return None,
    };

    // Create the _rn column symbol.
    let rn_col: Symbol = state.interner.intern("_rn");

    // Build PARTITION BY from outer refs.
    let mut partition_by: Vec<Symbol> = Vec::new();
    if let Some(current) = state.current() {
        let info = current.info(state);
        for &outer_ref in &info.outer_refs {
            let repr_col = current.repr.get(&outer_ref).copied().unwrap_or(outer_ref);
            partition_by.push(repr_col);
        }
    }

    // Create ROW_NUMBER() window function.
    let window_func = WindowFunc {
        kind: WindowKind::RowNumber,
        partition_by,
        order_by: sort_specs,
        frame: None,
        output: rn_col,
    };

    // Build: Window { input: sort_input, funcs: [ROW_NUMBER()] }
    let window = arena.alloc(Plan::Window {
        input: sort_input,
        funcs: vec![window_func],
    });

    // Build filter: _rn >= skip+1 AND _rn <= skip+fetch
    // For now, create a placeholder filter expression.
    // The actual comparison expressions will be created when the
    // THIR expression builder infrastructure is available.
    let rn_filter = arena.alloc_thir_expr(yelang_thir::ThirExpr::Literal(
        yelang_hir::hir::core::Lit::Unit,
    ));

    let filter = arena.alloc(Plan::Filter {
        input: window,
        pred: rn_filter,
    });

    Some(filter)
}

/// Check if a plan subtree matches the ORDER BY + LIMIT pattern.
///
/// Returns true if the node is a Limit whose input is a Sort.
pub(super) fn is_orderby_limit_pattern(
    node: PlanId,
    arena: &PlanArena,
) -> bool {
    match arena.plan(node) {
        Plan::Limit { input, .. } => {
            matches!(arena.plan(*input), Plan::Sort { .. })
        }
        _ => false,
    }
}
