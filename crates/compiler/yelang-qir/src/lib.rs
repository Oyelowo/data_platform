//! Query Intermediate Representation (QIR) for Yelang.
//!
//! This crate lowers typed HIR query and selector constructs into a logical
//! query plan, applies logical rewrites (including decorrelation), produces a
//! physical plan, and provides an execution interface backed by pluggable
//! storage backends.

pub mod backend;
pub mod errors;
pub mod exec;
pub mod expr;
pub mod ids;
pub mod logical;
pub mod physical;
pub mod rewrite;
pub mod util;

pub use errors::{LoweringError, PlanError, QirResult};

use yelang_hir::ids::{BodyId, QueryId};
use yelang_tycheck::tcx::TyCtxt;

/// Lower a typed HIR query to a logical QIR plan.
///
/// This is the main entry point for Phase I. The physical planner and executor
/// consume the returned `LogicalPlan`.
pub fn lower_query(
    tcx: &TyCtxt,
    body_id: BodyId,
    query_id: QueryId,
) -> QirResult<logical::LogicalPlan> {
    let mut plan = logical::LogicalPlan::empty();
    plan.lower_query(tcx, body_id, query_id)?;
    Ok(plan)
}
