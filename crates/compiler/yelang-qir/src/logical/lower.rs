//! Main HIR -> LIR lowering driver.
//!
//! Lowering is trait-driven: it reads `yelang_tycheck::typeck_results` to learn
//! which trait/method each HIR call resolved to. There is no method-name
//! pattern matching.

use yelang_hir::ids::{BodyId, DefId, QueryId};
use yelang_tycheck::tcx::TyCtxt;
use yelang_tycheck::typeck_results::TypeckResults;

use crate::errors::LoweringError;
use crate::ids::LirId;
use crate::logical::plan::LogicalPlan;

/// Context shared while lowering a single function body.
pub struct LoweringCtxt<'a> {
    pub tcx: &'a TyCtxt,
    pub body_id: BodyId,
    pub results: &'a TypeckResults,
}

impl<'a> LoweringCtxt<'a> {
    pub fn new(tcx: &'a TyCtxt, body_id: BodyId, results: &'a TypeckResults) -> Self {
        Self { tcx, body_id, results }
    }

    pub fn krate(&self) -> &yelang_hir::crate_data::Crate {
        self.tcx.crate_hir()
    }
}

/// Lower a typed HIR query into a logical plan.
pub fn lower_query(
    plan: &mut LogicalPlan,
    ctx: &LoweringCtxt<'_>,
    query_id: QueryId,
) -> Result<LirId, LoweringError> {
    let query = ctx.krate().query(query_id).ok_or(LoweringError::UnsupportedClause)?;
    super::lower_query::lower_query(plan, ctx, query)
}

/// Lower an arbitrary HIR expression that appears in query context.
pub fn lower_expr(
    plan: &mut LogicalPlan,
    ctx: &LoweringCtxt<'_>,
    expr_id: yelang_hir::ids::ExprId,
) -> Result<crate::ids::QExprId, LoweringError> {
    super::lower_expr::lower_hir_expr(plan, ctx, expr_id)
}

/// Resolve a method call to its trait/method DefIds.
pub fn resolve_method(
    ctx: &LoweringCtxt<'_>,
    _expr_id: yelang_hir::ids::ExprId,
) -> Option<MethodRes> {
    // TODO: wire up once TypeckResults stores method resolution.
    let _ = ctx;
    None
}

/// Check whether a method resolution is through a given lang-item trait.
pub fn is_lang_trait(_ctx: &LoweringCtxt<'_>, _trait_def: DefId, _lang: &str) -> bool {
    // TODO: wire up once lang items are exposed on Crate.
    false
}

/// Minimal method-resolution fact used by lowering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MethodRes {
    pub trait_def_id: Option<DefId>,
    pub method_def_id: DefId,
}

/// Return the lang-item trait names we care about.
pub struct LangTraits {
    pub iterator: Option<DefId>,
    pub into_iter: Option<DefId>,
    pub queryable: Option<DefId>,
    pub aggregate: Option<DefId>,
}

impl LangTraits {
    pub fn load(_ctx: &LoweringCtxt<'_>) -> Self {
        // TODO: read from ctx.tcx.lang_items once the API is stable.
        Self {
            iterator: None,
            into_iter: None,
            queryable: None,
            aggregate: None,
        }
    }
}
