//! Tests for logical rewrites.

use yelang_qir::expr::{QExpr, QLit};
use yelang_qir::logical::operator::ScanSource;
use yelang_qir::logical::plan::LogicalPlan;
use yelang_qir::rewrite::{NormalizePass, SimplifyPass, apply_rewrites, pass::RewritePass};
use yelang_ty::ty::TyId;

fn ty() -> TyId {
    TyId::new(1)
}

#[test]
fn rewrite_passes_return_false_on_empty_plan() {
    let mut plan = LogicalPlan::empty();
    assert!(!NormalizePass.run(&mut plan).unwrap());
    assert!(!SimplifyPass.run(&mut plan).unwrap());
}

#[test]
fn apply_rewrites_creates_root_for_empty_plan() {
    let mut plan = LogicalPlan::empty();
    let root = apply_rewrites(&mut plan).unwrap();
    assert!(plan.root.is_some());
    assert_eq!(plan.root, Some(root));
}

#[test]
fn apply_rewrites_preserves_existing_root() {
    let mut plan = LogicalPlan::empty();
    let source_expr = plan.alloc_expr(QExpr::Lit(QLit::Int(1), ty()));
    let scan = plan.scan(ScanSource::Expr(source_expr), ty());
    plan.set_root(scan);

    let root = apply_rewrites(&mut plan).unwrap();
    assert_eq!(root, scan);
}
