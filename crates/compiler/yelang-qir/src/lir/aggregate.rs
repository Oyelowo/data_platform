//! Lowering of `Aggregate` trait methods.
//!
//! Users normally do not call `Aggregate::init`/`iterate`/`merge`/`finalize`
//! directly; those are used by the generated physical plan. This module handles
//! the rare surface calls (e.g., a user-defined `classify()`) by falling back
//! to a scalar `MethodCall` expression. The actual aggregate *operator*
//! construction lives in `queryable.rs` because the surface syntax that produces
//! an aggregate plan node is `Queryable::aggregate` and its sugar (`sum`,
//! `avg`, `count`).

use yelang_hir::ids::{DefId, ExprId};
use yelang_ty::ty::TyId;

use crate::errors::LoweringError;
use crate::expr::QExpr;
use crate::ids::QExprId;
use crate::lir::lower::LoweringCtxt;
use crate::lir::lower::method::LoweredMethod;
use crate::lir::plan::LogicalPlan;

/// Lower a method call on the `Aggregate` trait.
pub fn lower(
    plan: &mut LogicalPlan,
    ctx: &mut LoweringCtxt<'_>,
    _expr_id: ExprId,
    method_def_id: Option<DefId>,
    receiver: ExprId,
    args: &[ExprId],
    ty: TyId,
) -> Result<LoweredMethod, LoweringError> {
    let recv = crate::lir::lower::expr::lower_hir_expr(plan, ctx, receiver)?;
    let lowered_args: Result<Vec<QExprId>, _> = args
        .iter()
        .map(|arg| crate::lir::lower::expr::lower_hir_expr(plan, ctx, *arg))
        .collect();
    let method = method_def_id.unwrap_or_else(|| DefId::new(1));
    Ok(LoweredMethod::Expr(plan.alloc_expr(QExpr::MethodCall {
        receiver: recv,
        method,
        args: lowered_args?,
        ty,
    })))
}
