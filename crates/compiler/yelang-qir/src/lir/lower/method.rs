//! Trait-driven method-call lowering.
//!
//! This module inspects the `MethodResolution` recorded by type checking and
//! dispatches to the appropriate lowering routine. Ordinary (non-query)
//! method calls fall back to a `QExpr::MethodCall`.

use yelang_hir::ids::{DefId, ExprId};
use yelang_interner::Symbol;
use yelang_ty::ty::TyId;

use crate::errors::LoweringError;
use crate::expr::QExpr;
use crate::ids::{LirId, QExprId};
use crate::lir::lower::{resolve_method, LoweringCtxt};
use crate::lir::plan::LogicalPlan;

/// Result of lowering a method call.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LoweredMethod {
    /// A normal scalar expression.
    Expr(QExprId),
    /// A subplan fragment (e.g., a `Queryable` pipeline or aggregate result).
    Plan(LirId, TyId),
}

/// Lower a HIR method call.
pub fn lower_method_call(
    plan: &mut LogicalPlan,
    ctx: &mut LoweringCtxt<'_>,
    expr_id: ExprId,
    receiver: ExprId,
    _method: Symbol,
    args: &[ExprId],
    ty: TyId,
) -> Result<LoweredMethod, LoweringError> {
    // Method resolutions are keyed by the method-call expression id.
    let res = resolve_method(ctx, expr_id);

    if let Some(res) = res {
        if let Some(trait_id) = res.trait_def_id {
            if Some(trait_id) == ctx.lang_traits.queryable {
                let lir = crate::lir::lower::queryable::lower(
                    plan, ctx, expr_id, res.method_def_id, receiver, args, ty,
                )?;
                return Ok(LoweredMethod::Plan(lir, ty));
            }
            if Some(trait_id) == ctx.lang_traits.aggregate {
                return crate::lir::aggregate::lower(
                    plan, ctx, expr_id, res.method_def_id, receiver, args, ty,
                );
            }
            if Some(trait_id) == ctx.lang_traits.iterator || Some(trait_id) == ctx.lang_traits.into_iter {
                return crate::lir::iterator::lower(
                    plan, ctx, expr_id, res.method_def_id, receiver, args, ty,
                );
            }
        }
    }

    // Fallback: ordinary method call expression.
    let recv = crate::lir::lower::expr::lower_hir_expr(plan, ctx, receiver)?;
    let lowered_args: Result<Vec<_>, _> = args
        .iter()
        .map(|arg| crate::lir::lower::expr::lower_hir_expr(plan, ctx, *arg))
        .collect();
    let method_def = res.and_then(|r| r.method_def_id).unwrap_or_else(|| DefId::new(1));
    Ok(LoweredMethod::Expr(plan.alloc_expr(QExpr::MethodCall {
        receiver: recv,
        method: method_def,
        args: lowered_args?,
        ty,
    })))
}
