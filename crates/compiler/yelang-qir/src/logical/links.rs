//! Lower `links` paths to correlated joins and `AttachField` operators.

use yelang_hir::hir::query::SelectLinkPath;
use yelang_interner::Symbol;

use crate::errors::LoweringError;
use crate::expr::QExpr;
use crate::ids::QirId;
use crate::logical::LogicalPlan;

/// Lower a `links` path rooted at `anchor` into a sub-plan that materializes
/// the nested collection as an `AttachField`.
///
/// The skeleton returns the anchor unchanged. The real implementation will
/// chain `LeftOuterJoin` + `AttachField` segments and resolve `_from`/`_to`
/// endpoint fields.
pub fn lower_links(
    _plan: &mut LogicalPlan,
    _anchor: QirId,
    _anchor_label: Symbol,
    _anchor_elem_ty: yelang_ty::ty::TyId,
    _path: &SelectLinkPath,
) -> Result<(QirId, QExpr), LoweringError> {
    Err(LoweringError::UnsupportedSelector)
}
