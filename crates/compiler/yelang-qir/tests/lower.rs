//! Lowering tests for `yelang-qir`.
//!
//! These tests currently exercise the skeleton entry point. As lowering is
//! implemented they will be expanded to assert operator shape for each query
//! construct.

use yelang_arena::DefId;
use yelang_hir::ids::{BodyId, QueryId};
use yelang_tycheck::tcx::TyCtxt;

#[test]
fn lower_query_skeleton_returns_plan() {
    // Build an empty TyCtxt. The skeleton does not inspect HIR, so this is
    // sufficient for the placeholder test.
    let crate_hir = yelang_hir::Crate::new(DefId::new(1));
    let tcx = TyCtxt::new(crate_hir);
    let body_id = BodyId::default();
    let query_id = QueryId::default();

    let plan = yelang_qir::lower_query(&tcx, body_id, query_id);
    assert!(plan.is_ok(), "lower_query should return Ok for the skeleton");

    let plan = plan.unwrap();
    assert!(plan.root.is_some(), "logical plan should have a root operator");
}
