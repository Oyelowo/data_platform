//! Tests for PIR construction and physical planning.

use yelang_qir::backend::{MemoryBackend, StreamBackend, supports_aggregate_op};
use yelang_qir::expr::{AggregateClass, QExprId};
use yelang_qir::lir::operator::AggregateOp;
use yelang_qir::pir::operator::{AggMode, PirOp};
use yelang_qir::pir::plan::PhysicalPlan;
use yelang_qir::pir::props::{Partitioning, PhysicalProps};
use yelang_ty::ty::TyId;

fn ty() -> TyId {
    TyId::new(1)
}

#[test]
fn physical_plan_alloc_and_root() {
    let mut plan = PhysicalPlan::empty();
    let op = PirOp::Values { rows: vec![] };
    let id = plan.alloc(op, PhysicalProps::any(), yelang_qir::pir::props::Cost::zero());
    plan.set_root(id);
    assert_eq!(plan.root, Some(id));
}

#[test]
fn memory_backend_supports_all_aggregate_classes() {
    let backend = MemoryBackend::new();
    let op = dummy_aggregate(AggregateClass::Distributive);
    assert!(supports_aggregate_op(&backend, &op));
    let op = dummy_aggregate(AggregateClass::Holistic);
    assert!(supports_aggregate_op(&backend, &op));
}

#[test]
fn stream_backend_rejects_holistic() {
    let backend = StreamBackend::new();
    let op = dummy_aggregate(AggregateClass::Holistic);
    assert!(!supports_aggregate_op(&backend, &op));
}

#[test]
fn scalar_distributive_aggregate_plan() {
    let mut plan = PhysicalPlan::empty();
    let input = plan.alloc(PirOp::Values { rows: vec![] }, PhysicalProps::any(), yelang_qir::pir::props::Cost::zero());
    let agg = yelang_qir::pir::operator::PhysicalAggregateOp {
        agg_def: yelang_arena::DefId::new(1),
        impl_def: yelang_arena::DefId::new(2),
        class: AggregateClass::Distributive,
        input_expr: QExprId(0),
        init: QExprId(0),
        step: QExprId(0),
        merge: QExprId(0),
        finish: QExprId(0),
        config: QExprId(0),
        acc_ty: ty(),
        out_ty: ty(),
    };
    let root = yelang_qir::pir::rules::aggregate::plan_scalar_aggregate(&mut plan, input, agg).unwrap();
    assert!(matches!(plan.operator(root), PirOp::HashAggregate { mode: AggMode::Final, .. }));
}

#[test]
fn pir_enforcer_inserts_gather_for_singleton() {
    let mut plan = PhysicalPlan::empty();
    let input = plan.alloc(PirOp::Values { rows: vec![] }, PhysicalProps::any(), yelang_qir::pir::props::Cost::zero());
    let required = PhysicalProps {
        ordering: Default::default(),
        partitioning: Partitioning::Singleton,
        location: Default::default(),
        boundedness: Default::default(),
    };
    let actual = PhysicalProps::any();
    let out = yelang_qir::pir::enforcers::enforce(&mut plan, input, &required, &actual).unwrap();
    assert!(matches!(plan.operator(out), PirOp::Exchange { .. }));
}

fn dummy_aggregate(class: AggregateClass) -> AggregateOp {
    AggregateOp {
        agg_def: yelang_arena::DefId::new(1),
        impl_def: yelang_arena::DefId::new(2),
        class,
        per_row: QExprId(0),
        init: QExprId(0),
        step: QExprId(0),
        merge: QExprId(0),
        finish: QExprId(0),
        config: QExprId(0),
        acc_ty: ty(),
        out_ty: ty(),
    }
}
