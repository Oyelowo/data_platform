//! Tests for closure-based evaluation of built-in aggregates.
//!
//! These tests construct logical plans directly and exercise the generic
//! aggregate closure protocol (`init`/`step`/`merge`/`finish`) for each
//! built-in marker.  They do not depend on the type checker being able to
//! resolve `Array: Queryable`; that integration is tested separately once
//! the stdlib impl is wired into trait resolution.

use yelang_interner::Symbol;
use yelang_qir::backend::MemoryBackend;
use yelang_qir::exec::{MemoryExecutor, QueryExecutor, Value};
use yelang_qir::expr::{AggregateClass, CastKind, QBinaryOp, QExpr, QExprId, QLit};
use yelang_qir::ids::BinderId;
use yelang_qir::logical::operator::AggregateOp;
use yelang_qir::logical::plan::LogicalPlan;
use yelang_qir::pir::planner::plan_logical;
use yelang_hir::ids::DefId;
use yelang_ty::ty::TyId;

fn ty() -> TyId {
    TyId::new(1)
}

fn int_lit(plan: &mut LogicalPlan, v: i128) -> QExprId {
    plan.alloc_expr(QExpr::Lit(QLit::Int(v), ty()))
}

fn float_lit(plan: &mut LogicalPlan, v: f64) -> QExprId {
    plan.alloc_expr(QExpr::Lit(QLit::Float(v), ty()))
}

fn col(plan: &mut LogicalPlan, b: BinderId) -> QExprId {
    plan.alloc_expr(QExpr::Column(b, ty()))
}

fn closure(plan: &mut LogicalPlan, param: BinderId, body: QExprId) -> QExprId {
    plan.alloc_expr(QExpr::Closure {
        params: vec![param],
        body,
        captures: vec![],
        ty: ty(),
    })
}

fn closure2(plan: &mut LogicalPlan, p1: BinderId, p2: BinderId, body: QExprId) -> QExprId {
    plan.alloc_expr(QExpr::Closure {
        params: vec![p1, p2],
        body,
        captures: vec![],
        ty: ty(),
    })
}

fn closure0(plan: &mut LogicalPlan, body: QExprId) -> QExprId {
    plan.alloc_expr(QExpr::Closure {
        params: vec![],
        body,
        captures: vec![],
        ty: ty(),
    })
}

fn field(plan: &mut LogicalPlan, base: QExprId, field_sym: Symbol) -> QExprId {
    plan.alloc_expr(QExpr::Field(base, field_sym, ty()))
}

fn record(plan: &mut LogicalPlan, fields: Vec<(Symbol, QExprId)>) -> QExprId {
    plan.alloc_expr(QExpr::Record(fields, ty()))
}

fn binary(plan: &mut LogicalPlan, op: QBinaryOp, l: QExprId, r: QExprId) -> QExprId {
    plan.alloc_expr(QExpr::Binary(op, l, r, ty()))
}

fn cast(plan: &mut LogicalPlan, e: QExprId, kind: CastKind) -> QExprId {
    plan.alloc_expr(QExpr::Cast(e, kind, ty()))
}

fn values_of_ints(plan: &mut LogicalPlan, values: &[i128]) -> yelang_qir::ids::LirId {
    let exprs: Vec<_> = values.iter().map(|&v| int_lit(plan, v)).collect();
    let id = plan.values(exprs, ty());
    plan.props[id].output_binder = Some(BinderId(1));
    id
}

fn values_of_floats(plan: &mut LogicalPlan, values: &[f64]) -> yelang_qir::ids::LirId {
    let exprs: Vec<_> = values.iter().map(|&v| float_lit(plan, v)).collect();
    let id = plan.values(exprs, ty());
    plan.props[id].output_binder = Some(BinderId(1));
    id
}

fn scalar_int(result: Value) -> i128 {
    match result {
        Value::Record(fields) => match &fields[..] {
            [(_, Value::Int(n))] => *n,
            _ => panic!("expected scalar int record, got {:?}", fields),
        },
        _ => panic!("expected scalar record, got {:?}", result),
    }
}

fn scalar_float(result: Value) -> f64 {
    match result {
        Value::Record(fields) => match &fields[..] {
            [(_, Value::Float(n))] => *n,
            _ => panic!("expected scalar float record, got {:?}", fields),
        },
        _ => panic!("expected scalar record, got {:?}", result),
    }
}

/// Build a bootstrap aggregate over a per-row binder/body pair.
/// `zero` is the initial accumulator value field; `one` is the multiplicative
/// identity when `op` is `Mul`.  For `Add` aggregates `one` is ignored.
fn make_value_aggregate(
    plan: &mut LogicalPlan,
    row_binder: BinderId,
    per_row_body: QExprId,
    op: QBinaryOp,
    zero: QExprId,
) -> AggregateOp {
    let v = Symbol::from(1000);
    let per_row = closure(plan, row_binder, per_row_body);

    let init_body = record(plan, vec![(v, zero)]);
    let init = closure0(plan, init_body);

    let acc = plan.fresh_binder();
    let item = plan.fresh_binder();
    let acc_col = col(plan, acc);
    let item_col = col(plan, item);
    let acc_value = field(plan, acc_col, v);
    let step_sum = binary(plan, op, acc_value, item_col);
    let step_body = record(plan, vec![(v, step_sum)]);
    let step = closure2(plan, acc, item, step_body);

    let a = plan.fresh_binder();
    let b = plan.fresh_binder();
    let a_col = col(plan, a);
    let b_col = col(plan, b);
    let a_value = field(plan, a_col, v);
    let b_value = field(plan, b_col, v);
    let merge_sum = binary(plan, op, a_value, b_value);
    let merge_body = record(plan, vec![(v, merge_sum)]);
    let merge = closure2(plan, a, b, merge_body);

    let finish_acc = plan.fresh_binder();
    let finish_acc_col = col(plan, finish_acc);
    let finish_body = field(plan, finish_acc_col, v);
    let finish = closure(plan, finish_acc, finish_body);

    AggregateOp {
        agg_def: DefId::new(1),
        impl_def: DefId::new(1),
        class: AggregateClass::Distributive,
        per_row,
        init,
        step,
        merge,
        finish,
        config: record(plan, vec![]),
        acc_ty: ty(),
        out_ty: ty(),
    }
}

fn count_aggregate(plan: &mut LogicalPlan, row_binder: BinderId, per_row_body: QExprId) -> AggregateOp {
    let count = Symbol::from(1002);
    let per_row = closure(plan, row_binder, per_row_body);

    let zero = int_lit(plan, 0);
    let init_body = record(plan, vec![(count, zero)]);
    let init = closure0(plan, init_body);

    let acc = plan.fresh_binder();
    let item = plan.fresh_binder();
    let acc_col = col(plan, acc);
    let count_field = field(plan, acc_col, count);
    let one = int_lit(plan, 1);
    let new_count = binary(plan, QBinaryOp::Add, count_field, one);
    let step_body = record(plan, vec![(count, new_count)]);
    let step = closure2(plan, acc, item, step_body);

    let a = plan.fresh_binder();
    let b = plan.fresh_binder();
    let a_col = col(plan, a);
    let b_col = col(plan, b);
    let a_count = field(plan, a_col, count);
    let b_count = field(plan, b_col, count);
    let merged = binary(plan, QBinaryOp::Add, a_count, b_count);
    let merge_body = record(plan, vec![(count, merged)]);
    let merge = closure2(plan, a, b, merge_body);

    let finish_acc = plan.fresh_binder();
    let finish_acc_col = col(plan, finish_acc);
    let finish_body = field(plan, finish_acc_col, count);
    let finish = closure(plan, finish_acc, finish_body);

    AggregateOp {
        agg_def: DefId::new(1),
        impl_def: DefId::new(1),
        class: AggregateClass::Distributive,
        per_row,
        init,
        step,
        merge,
        finish,
        config: record(plan, vec![]),
        acc_ty: ty(),
        out_ty: ty(),
    }
}

fn avg_aggregate(plan: &mut LogicalPlan, row_binder: BinderId, per_row_body: QExprId) -> AggregateOp {
    let sum = Symbol::from(1001);
    let count = Symbol::from(1002);
    let per_row = closure(plan, row_binder, per_row_body);

    let zero = int_lit(plan, 0);
    let init_body = record(plan, vec![(sum, zero), (count, zero)]);
    let init = closure0(plan, init_body);

    let acc = plan.fresh_binder();
    let item = plan.fresh_binder();
    let acc_col = col(plan, acc);
    let item_col = col(plan, item);
    let sum_field = field(plan, acc_col, sum);
    let count_field = field(plan, acc_col, count);
    let new_sum = binary(plan, QBinaryOp::Add, sum_field, item_col);
    let one = int_lit(plan, 1);
    let new_count = binary(plan, QBinaryOp::Add, count_field, one);
    let step_body = record(plan, vec![(sum, new_sum), (count, new_count)]);
    let step = closure2(plan, acc, item, step_body);

    let a = plan.fresh_binder();
    let b = plan.fresh_binder();
    let a_col = col(plan, a);
    let b_col = col(plan, b);
    let a_sum = field(plan, a_col, sum);
    let b_sum = field(plan, b_col, sum);
    let a_count = field(plan, a_col, count);
    let b_count = field(plan, b_col, count);
    let merged_sum = binary(plan, QBinaryOp::Add, a_sum, b_sum);
    let merged_count = binary(plan, QBinaryOp::Add, a_count, b_count);
    let merge_body = record(plan, vec![(sum, merged_sum), (count, merged_count)]);
    let merge = closure2(plan, a, b, merge_body);

    let finish_acc = plan.fresh_binder();
    let finish_acc_col = col(plan, finish_acc);
    let finish_sum_field = field(plan, finish_acc_col, sum);
    let finish_count_field = field(plan, finish_acc_col, count);
    let sum_float = cast(plan, finish_sum_field, CastKind::IntToFloat);
    let count_float = cast(plan, finish_count_field, CastKind::IntToFloat);
    let finish_body = binary(plan, QBinaryOp::Div, sum_float, count_float);
    let finish = closure(plan, finish_acc, finish_body);

    AggregateOp {
        agg_def: DefId::new(1),
        impl_def: DefId::new(1),
        class: AggregateClass::Algebraic,
        per_row,
        init,
        step,
        merge,
        finish,
        config: record(plan, vec![]),
        acc_ty: ty(),
        out_ty: ty(),
    }
}

fn run_aggregate(plan: &mut LogicalPlan, input: yelang_qir::ids::LirId, agg: AggregateOp) -> Value {
    let aggregated = plan.aggregate(input, agg, ty());
    plan.set_root(aggregated);
    let physical = plan_logical(plan, &MemoryBackend::new()).unwrap();
    MemoryExecutor::new().execute(&physical).unwrap()
}

#[test]
fn sum_i32() {
    let mut plan = LogicalPlan::empty();
    let values = values_of_ints(&mut plan, &[1, 2, 3, 4, 5]);
    let x = plan.fresh_binder();
    let x_col = col(&mut plan, x);
    let zero = int_lit(&mut plan, 0);
    let agg = make_value_aggregate(&mut plan, x, x_col, QBinaryOp::Add, zero);
    assert_eq!(scalar_int(run_aggregate(&mut plan, values, agg)), 15);
}

#[test]
fn sum_f64() {
    let mut plan = LogicalPlan::empty();
    let values = values_of_floats(&mut plan, &[1.5, 2.5, 3.0]);
    let x = plan.fresh_binder();
    let x_col = col(&mut plan, x);
    let zero = float_lit(&mut plan, 0.0);
    let agg = make_value_aggregate(&mut plan, x, x_col, QBinaryOp::Add, zero);
    assert!((scalar_float(run_aggregate(&mut plan, values, agg)) - 7.0).abs() < 1e-9);
}

#[test]
fn product_i32() {
    let mut plan = LogicalPlan::empty();
    let values = values_of_ints(&mut plan, &[2, 3, 4]);
    let x = plan.fresh_binder();
    let x_col = col(&mut plan, x);
    let one = int_lit(&mut plan, 1);
    let agg = make_value_aggregate(&mut plan, x, x_col, QBinaryOp::Mul, one);
    assert_eq!(scalar_int(run_aggregate(&mut plan, values, agg)), 24);
}

#[test]
fn product_f64() {
    let mut plan = LogicalPlan::empty();
    let values = values_of_floats(&mut plan, &[2.0, 3.0, 4.0]);
    let x = plan.fresh_binder();
    let x_col = col(&mut plan, x);
    let one = float_lit(&mut plan, 1.0);
    let agg = make_value_aggregate(&mut plan, x, x_col, QBinaryOp::Mul, one);
    assert!((scalar_float(run_aggregate(&mut plan, values, agg)) - 24.0).abs() < 1e-9);
}

#[test]
fn count_i32() {
    let mut plan = LogicalPlan::empty();
    let values = values_of_ints(&mut plan, &[10, 20, 30, 40]);
    let x = plan.fresh_binder();
    let x_col = col(&mut plan, x);
    let agg = count_aggregate(&mut plan, x, x_col);
    assert_eq!(scalar_int(run_aggregate(&mut plan, values, agg)), 4);
}

#[test]
fn avg_i32() {
    let mut plan = LogicalPlan::empty();
    let values = values_of_ints(&mut plan, &[10, 20, 30, 40]);
    let x = plan.fresh_binder();
    let x_col = col(&mut plan, x);
    let agg = avg_aggregate(&mut plan, x, x_col);
    assert!((scalar_float(run_aggregate(&mut plan, values, agg)) - 25.0).abs() < 1e-9);
}

#[test]
fn aggregate_after_filter_and_map() {
    let mut plan = LogicalPlan::empty();
    let values = values_of_ints(&mut plan, &[1, 2, 3, 4, 5, 6]);

    // filter: x -> x > 2
    let x = plan.fresh_binder();
    let x_col = col(&mut plan, x);
    let two = int_lit(&mut plan, 2);
    let pred_body = binary(&mut plan, QBinaryOp::Gt, x_col, two);
    let pred = closure(&mut plan, x, pred_body);
    let filtered = plan.filter(values, pred, ty());
    plan.props[filtered].output_binder = Some(x);

    // map: y -> y * 10
    let y = plan.fresh_binder();
    let y_col = col(&mut plan, y);
    let ten = int_lit(&mut plan, 10);
    let proj_body = binary(&mut plan, QBinaryOp::Mul, y_col, ten);
    let proj = closure(&mut plan, y, proj_body);
    let mapped = plan.map(filtered, proj, ty());
    plan.props[mapped].output_binder = Some(y);

    // sum
    let z = plan.fresh_binder();
    let z_col = col(&mut plan, z);
    let zero = int_lit(&mut plan, 0);
    let agg = make_value_aggregate(&mut plan, z, z_col, QBinaryOp::Add, zero);
    assert_eq!(scalar_int(run_aggregate(&mut plan, mapped, agg)), 180);
}
