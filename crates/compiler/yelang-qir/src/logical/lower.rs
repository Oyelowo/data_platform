//! Lower typed HIR query constructs to logical QIR.

use yelang_hir::ids::{BodyId, QueryId};
use yelang_tycheck::tcx::TyCtxt;

use crate::errors::LoweringError;
use crate::logical::LogicalPlan;
use crate::ids::QirId;

/// Lower a single typed HIR query into a logical QIR plan.
///
/// Phase I ships a skeleton that returns an empty plan for every query.
/// Incremental work will fill in lowering for `select`, `links`, `group by`,
/// and mutation queries.
pub fn lower_query(
    plan: &mut LogicalPlan,
    _tcx: &TyCtxt,
    _body_id: BodyId,
    _query_id: QueryId,
) -> Result<QirId, LoweringError> {
    // Skeleton: allocate a placeholder Expr(Error) root so the plan has a root.
    let root = plan.alloc_operator(crate::logical::operator::Operator::Expr(
        crate::expr::QExpr::Error,
    ));
    plan.set_root(root);
    Ok(root)
}
