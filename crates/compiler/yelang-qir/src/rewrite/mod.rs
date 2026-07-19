//! Logical rewrites over QIR plans.
//!
//! Rewrites are applied in a fixed-point loop until no rule makes progress.

use crate::errors::LoweringError;
use crate::ids::LirId;
use crate::logical::plan::LogicalPlan;
use crate::logical::props::LogicalProps;

pub mod decorrelate;
pub mod merge_maps;
pub mod normalize;
pub mod pass;
pub mod predicate_pushdown;
pub mod projection_pushdown;
pub mod push_filter;
pub mod push_project;
pub mod simplify;
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
        let id = plan.alloc_operator(crate::logical::operator::LirOp::Expr(expr), props);
        plan.set_root(id);
        id
    });

    let mut changed = true;
    while changed {
        changed = false;
        changed |= normalize::NormalizePass.run(plan)?;
        changed |= simplify::SimplifyPass.run(plan)?;
        changed |= push_filter::PushFilterPass.run(plan)?;
        changed |= push_project::PushProjectPass.run(plan)?;
        changed |= merge_maps::MergeMapsPass.run(plan)?;
        changed |= predicate_pushdown::PredicatePushdownPass.run(plan)?;
        changed |= projection_pushdown::ProjectionPushdownPass.run(plan)?;
    }

    Ok(root)
}
