//! Logical rewrites over QIR plans.
//!
//! Rewrites are applied in a fixed-point loop until no rule makes progress.

use yelang_arena::FxHashMap;

use crate::errors::LoweringError;
use crate::expr::QExpr;
use crate::ids::{BinderId, LirId, QExprId};
use crate::lir::plan::LogicalPlan;
use crate::lir::props::LogicalProps;

pub mod decorrelate;
pub mod decorrelate_agg;
pub mod decorrelate_rules;
pub mod decorrelate_window;
pub mod distinct;
pub mod elision;
pub mod equiv;
pub mod fold;
pub mod groupjoin;
pub mod merge_maps;
pub mod normalize;
pub mod pass;
pub mod predicate_pushdown;
pub mod projection_pushdown;
pub mod push_filter;
pub mod push_project;
pub mod pushdown;
pub mod simplify;
pub mod topn;
pub mod unnest_subqueries;

pub use pass::{RewritePass, apply_to_fixpoint};
pub use normalize::NormalizePass;
pub use simplify::SimplifyPass;
pub use merge_maps::MergeMapsPass;
pub use push_filter::PushFilterPass;
pub use push_project::PushProjectPass;
pub use predicate_pushdown::PredicatePushdownPass;
pub use projection_pushdown::ProjectionPushdownPass;
pub use decorrelate::DecorrelatePass;
pub use unnest_subqueries::UnnestSubqueriesPass;

/// Apply the standard normalization + optimization rewrite batch.
pub fn apply_rewrites(plan: &mut LogicalPlan) -> Result<LirId, LoweringError> {
    let root = plan.root.unwrap_or_else(|| {
        let ty = plan.exprs.get(crate::ids::QExprId(0)).map(|e| e.ty()).unwrap_or_else(|| yelang_ty::ty::TyId::new(1));
        let expr = plan.alloc_expr(crate::expr::QExpr::Error(ty));
        let props = LogicalProps::new(ty);
        let id = plan.alloc_operator(crate::lir::operator::LirOp::Expr(expr), props);
        plan.set_root(id);
        id
    });

    let mut changed = true;
    while changed {
        changed = false;
        changed |= normalize::NormalizePass.run(plan)?;
        changed |= simplify::SimplifyPass.run(plan)?;
        changed |= merge_maps::MergeMapsPass.run(plan)?;
        changed |= push_filter::PushFilterPass.run(plan)?;
        changed |= push_project::PushProjectPass.run(plan)?;
        changed |= predicate_pushdown::PredicatePushdownPass.run(plan)?;
        changed |= projection_pushdown::ProjectionPushdownPass.run(plan)?;
        // Unnest scalar subplans into joins, then flatten any correlated
        // dependent joins that resulted.
        changed |= unnest_subqueries::UnnestSubqueriesPass.run(plan)?;
        changed |= decorrelate::DecorrelatePass.run(plan)?;
    }

    Ok(plan.root.unwrap_or(root))
}

// -----------------------------------------------------------------------------
// Shared rewrite helpers
// -----------------------------------------------------------------------------

/// If `expr` is a single-parameter closure, return its parameter binder and body.
pub(crate) fn as_closure(plan: &LogicalPlan, expr: QExprId) -> Option<(BinderId, QExprId)> {
    match plan.expr(expr) {
        QExpr::Closure { params, body, .. } if params.len() == 1 => Some((params[0], *body)),
        _ => None,
    }
}

/// Return all operator ids reachable from the current plan root, in no
/// particular order.  Rewrites should only inspect reachable nodes so that
/// replaced / dead operators do not cause infinite loops in the fixpoint
/// driver.
pub(crate) fn reachable_ids(plan: &LogicalPlan) -> Vec<LirId> {
    let mut ids = Vec::new();
    let Some(root) = plan.root else { return ids };
    let mut stack = vec![root];
    let mut seen = std::collections::HashSet::new();
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        ids.push(id);
        if let Some(op) = plan.operators.get(id) {
            stack.extend(op.children());
        }
    }
    ids
}

/// Apply a map of operator rewrites (old LirId -> new LirId) to every child
/// reference in the plan and to the root.  Follows rewrite chains to a fixed
/// point, so `A -> B` followed by `B -> C` resolves to `C`.
pub(crate) fn apply_id_rewrites(plan: &mut LogicalPlan, rewrites: &FxHashMap<LirId, LirId>) {
    fn resolve(map: &FxHashMap<LirId, LirId>, mut id: LirId) -> LirId {
        while let Some(&next) = map.get(&id) {
            id = next;
        }
        id
    }

    let ids: Vec<LirId> = plan.operators.iter_enumerated().map(|(id, _)| id).collect();
    for id in ids {
        if let Some(op) = plan.operators.get_mut(id) {
            op.map_children(|child| resolve(rewrites, child));
        }
    }

    if let Some(root) = plan.root {
        plan.root = Some(resolve(rewrites, root));
    }
}

