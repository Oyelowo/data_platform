//! Tests for the execution layer.

use yelang_interner::Symbol;
use yelang_qir::exec::{
    ExecPlan, MemoryExecutor, QueryExecutor, RecordBatch, Value,
    exchange::route_batch,
    pipeline::build_exec,
    spill::spill_batches,
    value::ArrowSchema,
};
use yelang_qir::pir::operator::ExchangeKind;
use yelang_qir::pir::plan::PhysicalPlan;

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
