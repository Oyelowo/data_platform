//! Integration tests across LIR -> PIR -> Exec.

use yelang_interner::Symbol;
use yelang_qir::backend::MemoryBackend;
use yelang_qir::expr::{QExpr, QLit};
use yelang_qir::logical::operator::ScanSource;
use yelang_qir::logical::plan::LogicalPlan;
use yelang_qir::pir::planner::plan_logical;
use yelang_ty::ty::TyId;

fn ty() -> TyId {
    TyId::new(1)
}

#[test]
fn logical_to_physical_plan_runs() {
    let mut logical = LogicalPlan::empty();
    let _source_expr = logical.alloc_expr(QExpr::Lit(QLit::Int(1), ty()));
    let scan = logical.scan(ScanSource::Named(Symbol::from(1)), ty());
    let pred = logical.alloc_expr(QExpr::Lit(QLit::Bool(true), ty()));
    let filtered = logical.filter(scan, pred, ty());
    let proj = logical.alloc_expr(QExpr::Lit(QLit::Int(2), ty()));
    let mapped = logical.map(filtered, proj, ty());
    logical.set_root(mapped);

    let backend = MemoryBackend::new();
    let physical = plan_logical(&logical, &backend);
    assert!(physical.is_ok());
    let physical = physical.unwrap();
    assert!(physical.root.is_some());
}

#[test]
fn util_reachable_finds_all_operators() {
    use yelang_qir::ids::LirId;
    use yelang_qir::util::graph::reachable;

    let mut plan = LogicalPlan::empty();
    let source_expr = plan.alloc_expr(QExpr::Lit(QLit::Int(1), ty()));
    let scan = plan.scan(ScanSource::Expr(source_expr), ty());
    let pred = plan.alloc_expr(QExpr::Lit(QLit::Bool(true), ty()));
    let filtered = plan.filter(scan, pred, ty());
    plan.set_root(filtered);

    let reached = reachable(&plan, filtered);
    assert!(reached.contains(&filtered));
    assert!(reached.contains(&scan));
    assert!(!reached.contains(&LirId(999)));
}
