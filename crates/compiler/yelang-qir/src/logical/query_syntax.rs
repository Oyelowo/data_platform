//! Lower `ThirExpr::Query` to LIR using the HIR query side table.

use yelang_hir::ids::QueryId;

use crate::errors::LoweringError;
use crate::expr::QExprId;
use crate::lir::plan::LogicalPlan;

/// Lower a query-syntax expression (`select ... from ...`) to LIR.
pub fn lower_query_syntax(
    _plan: &mut LogicalPlan,
    _ctx: &mut super::ExtractCtxt<'_>,
    _query_id: QueryId,
) -> Result<QExprId, LoweringError> {
    // TODO(phase3): read HIR query side table and build identical LIR operators.
    Err(LoweringError::UnsupportedExpr)
}
