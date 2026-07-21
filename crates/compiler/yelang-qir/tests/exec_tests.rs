//! Tests for the execution layer.

use yelang_interner::Symbol;
use yelang_qir::backend::MemoryBackend;
use yelang_qir::exec::{
    ExecPlan, MemoryExecutor, QueryExecutor, RecordBatch, Value,
    exchange::route_batch,
    pipeline::build_exec,
    spill::spill_batches,
    value::ArrowSchema,
};
use yelang_qir::expr::{AggregateClass, QExpr, QLit};
use yelang_qir::lir::operator::AggregateOp;
use yelang_qir::lir::plan::LogicalPlan;
use yelang_qir::pir::operator::ExchangeKind;
use yelang_qir::pir::plan::PhysicalPlan;
use yelang_qir::pir::planner::plan_logical;
use yelang_hir::ids::DefId;
use yelang_ty::ty::TyId;

#[test]
fn memory_executor_returns_empty_array() {
    let plan = PhysicalPlan::empty();
    let executor = MemoryExecutor::new();
    let result = executor.execute(&plan).unwrap();
    assert_eq!(result, Value::Array(vec![]));
}

#[test]
fn exec_plan_alloc_and_root() {
    let mut plan = ExecPlan::empty();
    let op = yelang_qir::exec::operator::ExecOp::Expr(yelang_qir::exec::operator::ExprExec {
        expr: yelang_qir::ids::QExprId(0),
    });
    let id = plan.alloc(op);
    plan.set_root(id);
    assert_eq!(plan.root, Some(id));
}

#[test]
fn record_batch_single_column() {
    let name = Symbol::from(1);
    let batch = RecordBatch::single_column(name, vec![Value::Int(1), Value::Int(2)]);
    assert_eq!(batch.row_count, 2);
    assert_eq!(batch.columns.len(), 1);
}

#[test]
fn exchange_route_single_identity() {
    let batch = RecordBatch::empty();
    let routed = route_batch(batch.clone(), &ExchangeKind::Single, 1).unwrap();
    assert_eq!(routed.len(), 1);
}

#[test]
fn spill_batches_returns_file() {
    let batch = RecordBatch::empty();
    let files = spill_batches(vec![batch], 0).unwrap();
    assert_eq!(files.len(), 1);
}

#[test]
fn build_exec_from_empty_physical_plan_fails() {
    let plan = PhysicalPlan::empty();
    assert!(build_exec(&plan).is_err());
}

#[test]
fn arrow_schema_default() {
    let schema = ArrowSchema::default();
    assert!(schema.fields.is_empty());
}


fn ty() -> TyId {
    TyId::new(1)
}

fn int_lit(plan: &mut LogicalPlan, v: i128) -> yelang_qir::ids::QExprId {
    plan.alloc_expr(QExpr::Lit(QLit::Int(v), ty()))
}

fn record_ints(plan: &mut LogicalPlan, fields: Vec<(Symbol, i128)>) -> yelang_qir::ids::QExprId {
    let field_exprs: Vec<_> = fields
        .into_iter()
        .map(|(name, v)| (name, int_lit(plan, v)))
        .collect();
    plan.alloc_expr(QExpr::Record(field_exprs, ty()))
}

fn col(plan: &mut LogicalPlan, b: yelang_qir::ids::BinderId) -> yelang_qir::ids::QExprId {
    plan.alloc_expr(QExpr::Column(b, ty()))
}

fn closure(plan: &mut LogicalPlan, param: yelang_qir::ids::BinderId, body: yelang_qir::ids::QExprId) -> yelang_qir::ids::QExprId {
    plan.alloc_expr(QExpr::Closure {
        params: vec![param],
        body,
        captures: vec![],
        ty: ty(),
    })
}

fn closure2(plan: &mut LogicalPlan, p1: yelang_qir::ids::BinderId, p2: yelang_qir::ids::BinderId, body: yelang_qir::ids::QExprId) -> yelang_qir::ids::QExprId {
    plan.alloc_expr(QExpr::Closure {
        params: vec![p1, p2],
        body,
        captures: vec![],
        ty: ty(),
    })
}

fn closure0(plan: &mut LogicalPlan, body: yelang_qir::ids::QExprId) -> yelang_qir::ids::QExprId {
    plan.alloc_expr(QExpr::Closure {
        params: vec![],
        body,
        captures: vec![],
        ty: ty(),
    })
}

fn field(plan: &mut LogicalPlan, base: yelang_qir::ids::QExprId, field_sym: Symbol) -> yelang_qir::ids::QExprId {
    plan.alloc_expr(QExpr::Field(base, field_sym, ty()))
}

fn record(plan: &mut LogicalPlan, fields: Vec<(Symbol, yelang_qir::ids::QExprId)>) -> yelang_qir::ids::QExprId {
    plan.alloc_expr(QExpr::Record(fields, ty()))
}

fn binary(plan: &mut LogicalPlan, op: yelang_qir::expr::QBinaryOp, l: yelang_qir::ids::QExprId, r: yelang_qir::ids::QExprId) -> yelang_qir::ids::QExprId {
    plan.alloc_expr(QExpr::Binary(op, l, r, ty()))
}

/// Field symbol used for the `value` field of Sum/Product accumulator records.
fn value_sym() -> Symbol {
    Symbol::from(100)
}

/// Build a bootstrap Sum-like aggregate over the given per-row binder and body.
fn sum_aggregate_op(
    plan: &mut LogicalPlan,
    row_binder: yelang_qir::ids::BinderId,
    per_row_body: yelang_qir::ids::QExprId,
) -> AggregateOp {
    let per_row = closure(plan, row_binder, per_row_body);

    let v = value_sym();
    let zero = int_lit(plan, 0);
    let init_body = record(plan, vec![(v, zero)]);
    let init = closure0(plan, init_body);

    let acc = plan.fresh_binder();
    let item = plan.fresh_binder();
    let acc_col = col(plan, acc);
    let item_col = col(plan, item);
    let acc_value = field(plan, acc_col, v);
    let step_sum = binary(plan, yelang_qir::expr::QBinaryOp::Add, acc_value, item_col);
    let step_body = record(plan, vec![(v, step_sum)]);
    let step = closure2(plan, acc, item, step_body);

    let a = plan.fresh_binder();
    let b = plan.fresh_binder();
    let a_col = col(plan, a);
    let b_col = col(plan, b);
    let a_value = field(plan, a_col, v);
    let b_value = field(plan, b_col, v);
    let merge_sum = binary(plan, yelang_qir::expr::QBinaryOp::Add, a_value, b_value);
    let merge_body = record(plan, vec![(v, merge_sum)]);
    let merge = closure2(plan, a, b, merge_body);

    let acc = plan.fresh_binder();
    let acc_col2 = col(plan, acc);
    let finish_body = field(plan, acc_col2, v);
    let finish = closure(plan, acc, finish_body);

    let config = record(plan, vec![]);

    AggregateOp {
        agg_def: DefId::new(1),
        impl_def: DefId::new(1),
        class: AggregateClass::Distributive,
        per_row,
        init,
        step,
        merge,
        finish,
        config,
        acc_ty: ty(),
        out_ty: ty(),
    }
}

#[test]
fn end_to_end_filter_project_slice() {
    let mut logical = LogicalPlan::empty();

    // Values: [1, 2, 3, 4, 5]
    let value_exprs: Vec<_> = (1..=5).map(|v| int_lit(&mut logical, v)).collect();
    let values = logical.values(value_exprs, ty());
    logical.props[values].output_binder = Some(yelang_qir::ids::BinderId(1));

    // filter: x -> x > 2
    let x = logical.fresh_binder();
    let two = int_lit(&mut logical, 2);
    let cx = col(&mut logical, x);
    let pred_body = logical.alloc_expr(QExpr::Binary(
        yelang_qir::expr::QBinaryOp::Gt,
        cx,
        two,
        ty(),
    ));
    let pred = closure(&mut logical, x, pred_body);
    let filtered = logical.filter(values, pred, ty());
    logical.props[filtered].output_binder = Some(x);

    // map: y -> y + 10
    let y = logical.fresh_binder();
    let ten = int_lit(&mut logical, 10);
    let cy = col(&mut logical, y);
    let map_body = logical.alloc_expr(QExpr::Binary(
        yelang_qir::expr::QBinaryOp::Add,
        cy,
        ten,
        ty(),
    ));
    let proj = closure(&mut logical, y, map_body);
    let mapped = logical.map(filtered, proj, ty());
    logical.set_root(mapped);

    let physical = plan_logical(&logical, &MemoryBackend::new()).unwrap();
    let executor = MemoryExecutor::new();
    let result = executor.execute(&physical).unwrap();

    // Filter (>2): [3,4,5]; map (+10): [13,14,15]
    match result {
        Value::Array(rows) => {
            let ints: Vec<i128> = rows
                .into_iter()
                .map(|v| match v {
                    Value::Int(n) => n,
                    _ => panic!("expected int"),
                })
                .collect();
            assert_eq!(ints, vec![13, 14, 15]);
        }
        _ => panic!("expected array result"),
    }
}

#[test]
fn end_to_end_scalar_aggregate() {
    let mut logical = LogicalPlan::empty();

    // Values: [1, 2, 3, 4, 5]
    let value_exprs: Vec<_> = (1..=5).map(|v| int_lit(&mut logical, v)).collect();
    let values = logical.values(value_exprs, ty());
    logical.props[values].output_binder = Some(yelang_qir::ids::BinderId(1));

    let x = logical.fresh_binder();
    let per_row_body = col(&mut logical, x);
    let agg = sum_aggregate_op(&mut logical, x, per_row_body);
    let aggregated = logical.aggregate(values, agg, ty());
    logical.set_root(aggregated);

    let physical = plan_logical(&logical, &MemoryBackend::new()).unwrap();
    let executor = MemoryExecutor::new();
    let result = executor.execute(&physical).unwrap();

    match result {
        Value::Record(fields) => {
            assert_eq!(fields.len(), 1);
            assert_eq!(fields[0].1, Value::Int(15));
        }
        _ => panic!("expected scalar aggregate record, got {:?}", result),
    }
}

#[test]
fn end_to_end_group_by() {
    let mut logical = LogicalPlan::empty();

    // Values: [{a: 1, b: 10}, {a: 1, b: 20}, {a: 2, b: 30}]
    let a_sym = Symbol::from(1);
    let b_sym = Symbol::from(2);
    let rows: Vec<_> = [(1, 10), (1, 20), (2, 30)]
        .iter()
        .map(|(a, b)| record_ints(&mut logical, vec![(a_sym, *a as i128), (b_sym, *b as i128)]))
        .collect();
    let values = logical.values(rows, ty());
    logical.props[values].output_binder = Some(yelang_qir::ids::BinderId(1));

    let x = logical.fresh_binder();
    let cx = col(&mut logical, x);
    let key_body = logical.alloc_expr(QExpr::Field(cx, a_sym, ty()));
    let key = closure(&mut logical, x, key_body);
    let grouped = logical.group_by(values, key, ty(), Symbol::from(3), ty());
    logical.set_root(grouped);

    let physical = plan_logical(&logical, &MemoryBackend::new()).unwrap();
    let executor = MemoryExecutor::new();
    let result = executor.execute(&physical).unwrap();

    let groups = result.try_into_array().expect("expected array of groups");
    assert_eq!(groups.len(), 2);
}

#[test]
fn end_to_end_hash_aggregate_group_by() {
    let mut logical = LogicalPlan::empty();

    // Values: [{a: 1, b: 10}, {a: 1, b: 20}, {a: 2, b: 30}]
    let a_sym = Symbol::from(1);
    let b_sym = Symbol::from(2);
    let rows: Vec<_> = [(1, 10), (1, 20), (2, 30)]
        .iter()
        .map(|(a, b)| record_ints(&mut logical, vec![(a_sym, *a as i128), (b_sym, *b as i128)]))
        .collect();
    let values = logical.values(rows, ty());
    logical.props[values].output_binder = Some(yelang_qir::ids::BinderId(1));

    let x = logical.fresh_binder();
    let cx = col(&mut logical, x);
    let group_key_body = logical.alloc_expr(QExpr::Field(cx, a_sym, ty()));
    let group_key = closure(&mut logical, x, group_key_body);

    let per_row_body = logical.alloc_expr(QExpr::Field(cx, b_sym, ty()));
    let agg = sum_aggregate_op(&mut logical, x, per_row_body);
    let aggregated = logical.aggregate_group_by(values, vec![group_key], vec![agg], ty());
    logical.set_root(aggregated);

    let physical = plan_logical(&logical, &MemoryBackend::new()).unwrap();
    let executor = MemoryExecutor::new();
    let result = executor.execute(&physical).unwrap();

    let rows = result.try_into_array().expect("expected array of aggregate rows");
    assert_eq!(rows.len(), 2);
}
