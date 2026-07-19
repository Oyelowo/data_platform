//! Lower HIR selector chains (`users@u[*].id`) into LIR.

use yelang_hir::hir::expr::{ComprehensionKind, Expr};

use crate::errors::LoweringError;
use crate::ids::LirId;
use crate::logical::lower::LoweringCtxt;
use crate::logical::plan::LogicalPlan;

/// Lower a comprehension / selector expression into LIR.
pub fn lower_comprehension(
    plan: &mut LogicalPlan,
    ctx: &LoweringCtxt<'_>,
    expr_id: yelang_hir::ids::ExprId,
) -> Result<LirId, LoweringError> {
    let expr = ctx.krate().expr(expr_id).ok_or(LoweringError::UnsupportedSelector)?;

    let Expr::Comprehension { kind, element, variables, condition } = expr else {
        return Err(LoweringError::UnsupportedSelector);
    };

    let _ = kind; // List/Set/Dict — all map to Queryable for optimization.

    // Lower the source chain.
    let mut input: Option<LirId> = None;
    for var in variables {
        let source_ty = ctx.results.expr_ty(var.source).unwrap_or_else(|| yelang_ty::ty::TyId::new(1));
        let source_expr = super::lower_expr::lower_hir_expr(plan, ctx, var.source)?;
        let scan = plan.scan(crate::logical::operator::ScanSource::Expr(source_expr), source_ty);
        input = Some(match input {
            Some(_prev) => {
                // TODO: build a FlatMap that iterates prev and yields scan.
                scan
            }
            None => scan,
        });
    }

    let mut input = input.ok_or(LoweringError::UnsupportedSelector)?;

    if let Some(cond) = condition {
        let pred = super::lower_expr::lower_hir_expr(plan, ctx, *cond)?;
        let out_ty = plan.props[input].output_ty;
        input = plan.filter(input, pred, out_ty);
    }

    let proj = super::lower_expr::lower_hir_expr(plan, ctx, *element)?;
    let out_ty = plan.expr(proj).ty();
    input = plan.map(input, proj, out_ty);

    Ok(input)
}

/// Lower a comprehension whose result kind matters.
pub fn lower_comprehension_kind(
    plan: &mut LogicalPlan,
    ctx: &LoweringCtxt<'_>,
    expr_id: yelang_hir::ids::ExprId,
) -> Result<(LirId, ComprehensionKind), LoweringError> {
    let expr = ctx.krate().expr(expr_id).ok_or(LoweringError::UnsupportedSelector)?;
    let Expr::Comprehension { kind, .. } = expr else {
        return Err(LoweringError::UnsupportedSelector);
    };
    let id = lower_comprehension(plan, ctx, expr_id)?;
    Ok((id, *kind))
}
