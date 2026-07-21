//! Lowering of `Iterator` and `IntoIterator` trait methods.
//!
//! For the first cut we treat iterator methods as ordinary scalar calls. Once
//! `Queryable` and `Iterator` are unified in the type system, this module will
//! lower `into_iter` and lazy iterator adapters into the same LIR operators as
//! their `Queryable` counterparts.

use yelang_hir::ids::{DefId, ExprId};
use yelang_ty::ty::TyId;

use crate::errors::LoweringError;
use crate::expr::QExpr;
use crate::ids::QExprId;
use crate::lir::lower::LoweringCtxt;
use crate::lir::lower::method::LoweredMethod;
use crate::lir::plan::LogicalPlan;

/// Lower an `Iterator` or `IntoIterator` method call.
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
