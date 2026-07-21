//! Lowering `links` graph traversal paths into LIR joins or edge expansions.

use yelang_hir::ids::DefId;
use yelang_interner::Symbol;
use yelang_ty::ty::TyId;

use crate::errors::LoweringError;
use crate::expr::QExprId;
use crate::ids::LirId;
use crate::lir::operator::EdgeDirection;
use crate::lir::plan::LogicalPlan;

/// A single step in a `links` path.
#[derive(Clone, Debug)]
pub struct LinkStep {
    pub edge: DefId,
    pub edge_alias: Symbol,
    pub direction: EdgeDirection,
    pub target: DefId,
    pub target_alias: Symbol,
    pub predicate: Option<QExprId>,
    pub min_hops: Option<usize>,
    pub max_hops: Option<usize>,
}

/// A parsed `links` path (possibly with variable length).
#[derive(Clone, Debug)]
pub struct LinkPath {
    pub steps: Vec<LinkStep>,
}

/// Lower a link path into LIR.
///
/// Direct links lower to `EdgeExpand` if the backend exposes an edge index,
/// otherwise to a join against the edge relation.
pub fn lower_link_path(
    _plan: &mut LogicalPlan,
    _input: LirId,
    _path: LinkPath,
    _out_ty: TyId,
) -> Result<LirId, LoweringError> {
    // Stub: real implementation expands steps into EdgeExpand / Join.
    Err(LoweringError::UnsupportedClause)
}
