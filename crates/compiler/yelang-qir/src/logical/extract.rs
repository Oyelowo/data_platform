//! Main THIR → LIR extraction pass.

use yelang_thir::{ThirBodyId, ThirExpr, ThirExprId};
use yelang_tycheck::tcx::TyCtxt;
use yelang_tycheck::typeck_results::TypeckResults;

use crate::errors::{LoweringError, QirResult};
use crate::ids::QExprId;
use crate::lir::plan::LogicalPlan;
use crate::rewrite;

use super::context::{ExtractCtxt, ThirView};
use super::convert::{expr_to_lir, lower_scalar_expr};

/// Lower a typed THIR body to a QIR logical plan.
pub fn lower_thir_body(
    tcx: &TyCtxt,
    thir: ThirView<'_>,
    body_id: ThirBodyId,
    results: &TypeckResults,
) -> QirResult<LogicalPlan> {
    let mut plan = LogicalPlan::empty();
    let mut ctx = ExtractCtxt::new(tcx, thir, results)?;
    let body = ctx.thir.bodies.bodies.get(body_id).ok_or(LoweringError::UnsupportedExpr)?;
    let root_expr = extract_expr(&mut plan, &mut ctx, body.value)?;
    let root_lir = expr_to_lir(&mut plan, root_expr)?;
    plan.set_root(root_lir);
    rewrite::apply_rewrites(&mut plan)?;
    Ok(plan)
}

/// Lower a single THIR expression to a QExpr.
pub fn extract_expr(
    plan: &mut LogicalPlan,
    ctx: &mut ExtractCtxt<'_>,
    expr: ThirExprId,
) -> Result<QExprId, LoweringError> {
    match ctx.thir.exprs.get(expr) {
        Some(ThirExpr::Call { func, args }) => {
            // TODO(phase3): detect Queryable method calls.
            lower_scalar_call(plan, ctx, *func, args)
        }
        Some(ThirExpr::Intrinsic { name, args }) => {
            // TODO(phase3): dispatch recognized query intrinsics.
            let _ = (name, args);
            lower_scalar_expr(plan, ctx, expr)
        }
        Some(ThirExpr::Query(query_id)) => {
            // TODO(phase3): lower query syntax from THIR.
            let _ = query_id;
            lower_scalar_expr(plan, ctx, expr)
        }
        Some(_) | None => lower_scalar_expr(plan, ctx, expr),
    }
}

fn lower_scalar_call(
    plan: &mut LogicalPlan,
    ctx: &mut ExtractCtxt<'_>,
    func: ThirExprId,
    args: &[ThirExprId],
) -> Result<QExprId, LoweringError> {
    let _ = (func, args);
    lower_scalar_expr(plan, ctx, func)
}
