//! Resolution of `Aggregate` trait impls from THIR aggregate arguments.

use yelang_arena::DefId;
use yelang_thir::ThirExprId;

use crate::errors::LoweringError;
use crate::expr::QExprId;
use crate::ids::LirId;
use crate::lir::plan::LogicalPlan;

/// Resolve an aggregate config expression (e.g. `Sum {}`) to its config def_id.
pub fn resolve_aggregate_config(
    _ctx: &super::ExtractCtxt<'_>,
    _agg_arg: ThirExprId,
) -> Result<(DefId, ThirExprId), LoweringError> {
    // TODO(phase3): resolve struct literal/path to aggregate config def_id.
    Err(LoweringError::UnsupportedExpr)
}

/// Extract an `AggregateOp` from a selected aggregate impl.
pub fn lower_aggregate(
    _plan: &mut LogicalPlan,
    _ctx: &mut super::ExtractCtxt<'_>,
    _input: LirId,
    _agg_arg: ThirExprId,
) -> Result<QExprId, LoweringError> {
    // TODO(phase3): build AggregateOp from AggregateImplInfo.
    Err(LoweringError::UnsupportedExpr)
}
