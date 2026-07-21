//! Tests for LIR construction and properties.

use yelang_interner::Symbol;
use yelang_qir::expr::{QExpr, QExprId, QLit};
use yelang_qir::lir::operator::{ConstructKind, JoinKind, LirOp, ScanSource};
use yelang_qir::lir::plan::LogicalPlan;
use yelang_qir::lir::props::{Boundedness, CardinalityClass};
use yelang_ty::ty::TyId;

fn ty() -> TyId {
    TyId::new(1)
}

#[test]
fn logical_plan_scan_and_filter() {
    let mut plan = LogicalPlan::empty();
    let source_expr = plan.alloc_expr(QExpr::Lit(QLit::Int(1), ty()));
    let scan = plan.scan(ScanSource::Expr(source_expr), ty());
    let pred = plan.alloc_expr(QExpr::Lit(QLit::Bool(true), ty()));
    let filtered = plan.filter(scan, pred, ty());
    plan.set_root(filtered);

    assert!(plan.root.is_some());
    assert_eq!(plan.props[scan].cardinality, CardinalityClass::Many);
}

#[test]
fn logical_plan_map_preserves_properties() {
    let mut plan = LogicalPlan::empty();
    let source_expr = plan.alloc_expr(QExpr::Lit(QLit::Int(1), ty()));
    let scan = plan.scan(ScanSource::Expr(source_expr), ty());
    let proj = plan.alloc_expr(QExpr::Lit(QLit::Int(2), ty()));
    let mapped = plan.map(scan, proj, ty());

    assert_eq!(plan.props[mapped].bounded, Boundedness::Bounded);
    assert_eq!(plan.props[mapped].cardinality, CardinalityClass::Many);
}

#[test]
fn logical_plan_aggregate_sets_cardinality_one() {
    let mut plan = LogicalPlan::empty();
    let source_expr = plan.alloc_expr(QExpr::Lit(QLit::Int(1), ty()));
    let scan = plan.scan(ScanSource::Expr(source_expr), ty());
    let unit = plan.alloc_expr(QExpr::Record(vec![], ty()));
    let agg = yelang_qir::lir::operator::AggregateOp {
        agg_def: yelang_arena::DefId::new(1),
        impl_def: yelang_arena::DefId::new(2),
        class: yelang_qir::expr::AggregateClass::Distributive,
        per_row: plan.alloc_expr(QExpr::Lit(QLit::Int(1), ty())),
        init: unit,
        step: unit,
        merge: unit,
        finish: unit,
        config: unit,
        acc_ty: ty(),
        out_ty: ty(),
    };
    let reduced = plan.aggregate(scan, agg, ty());
    assert_eq!(plan.props[reduced].cardinality, CardinalityClass::One);
}

#[test]
fn lir_op_children_match_structure() {
    use yelang_qir::ids::LirId;

    let op = LirOp::Filter {
        input: LirId(0),
        predicate: QExprId(0),
    };
    assert_eq!(op.children(), vec![LirId(0)]);

    let join = LirOp::Join {
        kind: JoinKind::Inner,
        left: LirId(0),
        right: LirId(1),
        predicate: None,
    };
    assert_eq!(join.children(), vec![LirId(0), LirId(1)]);
}

#[test]
fn logical_plan_construct_builds_record() {
    let mut plan = LogicalPlan::empty();
    let source_expr = plan.alloc_expr(QExpr::Lit(QLit::Int(1), ty()));
    let a = plan.scan(ScanSource::Expr(source_expr), ty());
    let b = plan.scan(ScanSource::Named(Symbol::from(1)), ty());
    let c = plan.construct(ConstructKind::Record, vec![(Symbol::from(1), a), (Symbol::from(2), b)], ty());
    assert!(matches!(plan.operator(c), LirOp::Construct { .. }));
}
