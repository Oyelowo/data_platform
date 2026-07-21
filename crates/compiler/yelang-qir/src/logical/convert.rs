//! Helpers for converting THIR expressions and subplans to QIR expressions.

use yelang_thir::ThirExprId;

use crate::errors::LoweringError;
use crate::expr::QExpr;
use crate::ids::{BinderId, LirId, QExprId};
use crate::lir::plan::LogicalPlan;

/// Lower an arbitrary THIR expression to a scalar QExpr.
pub fn lower_scalar_expr(
    _plan: &mut LogicalPlan,
    _ctx: &mut super::ExtractCtxt<'_>,
    _expr: ThirExprId,
) -> Result<QExprId, LoweringError> {
    // TODO(phase3): full THIR -> QExpr lowering.
    Err(LoweringError::UnsupportedExpr)
}

/// Convert a QExpr that wraps a subplan into the underlying LirId.
pub fn expr_to_lir(
    plan: &mut LogicalPlan,
    expr: QExprId,
) -> Result<LirId, LoweringError> {
    match plan.expr(expr) {
        QExpr::Subplan(lir, _) => Ok(*lir),
        _ => Err(LoweringError::UnsupportedExpr),
    }
}

/// Allocate a fresh binder for the output of an operator.
pub fn fresh_output_binder(plan: &mut LogicalPlan) -> BinderId {
    plan.fresh_binder()
}
