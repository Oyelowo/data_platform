//! Tests for the Phase 3 THIR → LIR extractor for `Queryable` method-call
//! pipelines.

use yelang_driver::Driver;
use yelang_qir::lir::operator::LirOp;

fn plan_for_function(src: &str, name: &str) -> yelang_qir::lir::plan::LogicalPlan {
    let full = format!("{}\nfn main() {{}}", src);
    let compiled = Driver::new()
        .compile_or_eval_main(&full)
        .expect("compile source");
    compiled
        .lower_thir_function(name)
        .expect("lower function to QIR")
}

fn root_op(plan: &yelang_qir::lir::plan::LogicalPlan) -> &LirOp {
    let root_id = plan.root.expect("plan should have a root");
    plan.operator(root_id)
}

fn child_input<'a>(plan: &'a yelang_qir::lir::plan::LogicalPlan, op: &'a LirOp) -> &'a LirOp {
    let children = op.children();
    assert!(!children.is_empty(), "operator should have a child input");
    plan.operator(children[0])
}

#[test]
fn query_array_sum_produces_aggregate() {
    let src = r#"
        fn test(q: QueryArray<i32>) -> i32 {
            q.sum()
        }
    "#;
    let plan = plan_for_function(src, "test");
    let op = root_op(&plan);
    let LirOp::Aggregate { agg, .. } = op else {
        panic!("expected Aggregate root, got {:?}", op);
    };
    assert_eq!(agg.class, yelang_qir::expr::AggregateClass::Distributive);
}

#[test]
fn filter_map_sum_chain() {
    let src = r#"
        fn test(q: QueryArray<i32>) -> i32 {
            q.filter(|x| x > 2).map(|x| x + 10).sum()
        }
    "#;
    let plan = plan_for_function(src, "test");

    // Root: Aggregate
    let root = root_op(&plan);
    let LirOp::Aggregate { .. } = root else {
        panic!("expected Aggregate root, got {:?}", root);
    };

    // Aggregate input: Map
    let map_op = child_input(&plan, root);
    let LirOp::Map { .. } = map_op else {
        panic!("expected Map, got {:?}", map_op);
    };

    // Map input: Filter
    let filter_op = child_input(&plan, map_op);
    let LirOp::Filter { .. } = filter_op else {
        panic!("expected Filter, got {:?}", filter_op);
    };

    // Filter input: Scan from the QueryArray parameter.
    let scan_op = child_input(&plan, filter_op);
    let LirOp::Scan { .. } = scan_op else {
        panic!("expected Scan, got {:?}", scan_op);
    };
}

#[test]
fn take_produces_slice() {
    let src = r#"
        fn test(q: QueryArray<i32>) -> Queryable<i32> {
            q.take(3)
        }
    "#;
    let plan = plan_for_function(src, "test");
    let op = root_op(&plan);
    let LirOp::Slice { limit, .. } = op else {
        panic!("expected Slice root, got {:?}", op);
    };
    assert!(limit.is_some(), "take should produce a limit");
}

#[test]
fn skip_produces_slice() {
    let src = r#"
        fn test(q: QueryArray<i32>) -> Queryable<i32> {
            q.skip(2)
        }
    "#;
    let plan = plan_for_function(src, "test");
    let op = root_op(&plan);
    let LirOp::Slice { limit, .. } = op else {
        panic!("expected Slice root, got {:?}", op);
    };
    assert!(limit.is_none(), "skip should not produce a limit");
}

#[test]
fn order_by_take_is_valid() {
    let src = r#"
        fn test(q: QueryArray<i32>) -> Queryable<i32> {
            q.order_by(|x| x).take(5)
        }
    "#;
    let plan = plan_for_function(src, "test");

    let root = root_op(&plan);
    let LirOp::Slice { .. } = root else {
        panic!("expected Slice root, got {:?}", root);
    };

    let order_op = child_input(&plan, root);
    let LirOp::OrderBy { .. } = order_op else {
        panic!("expected OrderBy, got {:?}", order_op);
    };
}
