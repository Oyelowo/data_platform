//! Query Intermediate Representation (QIR) for Yelang.
//!
//! This crate lowers typed HIR query and selector constructs into a logical
//! query plan (LIR), applies logical rewrites (including decorrelation),
//! produces a physical plan (PIR), and provides an execution interface backed
//! by pluggable storage backends.

pub mod backend;
pub mod demand;
pub mod errors;
pub mod exec;
pub mod expr;
pub mod ids;
pub mod logical;
pub mod lir;
pub mod pir;
pub mod rewrite;
pub mod util;
pub mod volatility;

pub use errors::{LoweringError, PlanError, QirError, QirResult};

use yelang_hir::ids::{BodyId, QueryId};
use yelang_tycheck::tcx::TyCtxt;
use yelang_tycheck::typeck_results::TypeckResults;

use crate::lir::lower::LoweringCtxt;
use crate::lir::plan::LogicalPlan;

/// Lower a typed HIR query to a logical QIR plan.
///
/// This is the main entry point for Phase I. The physical planner and executor
/// consume the returned `LogicalPlan`.
pub fn lower_query(
    tcx: &TyCtxt,
    body_id: BodyId,
    query_id: QueryId,
    results: &TypeckResults,
) -> QirResult<LogicalPlan> {
    let mut plan = LogicalPlan::empty();
    let mut ctx = LoweringCtxt::new(tcx, body_id, results);
    ctx.populate_stdlib_tables()?;
    lir::lower::populate_local_values(&mut plan, &mut ctx, body_id)?;
    lir::lower::lower_query(&mut plan, &mut ctx, query_id)?;
    rewrite::apply_rewrites(&mut plan)?;
    Ok(plan)
}

/// Plan a logical plan into a physical plan for the given backend.
pub fn plan_logical(
    logical: &LogicalPlan,
    backend: &dyn pir::capability::BackendCapability,
) -> QirResult<pir::PhysicalPlan> {
    Ok(pir::planner::plan_logical(logical, backend)?)
}
