//! Tests for aggregate resolution through the Phase 3 THIR → LIR extractor.
//!
//! These tests verify that `Sum`, `Count`, `Avg`, `Min`, and `Max` resolve to
//! the correct `AggregateClass` via their `Aggregate` trait impls.

use yelang_driver::Driver;
use yelang_qir::expr::AggregateClass;
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

fn root_aggregate(plan: &yelang_qir::lir::plan::LogicalPlan) -> &yelang_qir::lir::operator::AggregateOp {
    let root_id = plan.root.expect("plan should have a root");
    let op = plan.operator(root_id);
    let LirOp::Aggregate { agg, .. } = op else {
        panic!("expected Aggregate root, got {:?}", op);
    };
    agg
}

#[test]
fn sum_is_distributive() {
    let src = r#"
        fn test(q: QueryArray<i32>) -> i32 {
            q.sum()
        }
    "#;
    let plan = plan_for_function(src, "test");
    assert_eq!(root_aggregate(&plan).class, AggregateClass::Distributive);
}

#[test]
fn count_is_distributive() {
    let src = r#"
        fn test(q: QueryArray<i32>) -> usize {
            q.count()
        }
    "#;
    let plan = plan_for_function(src, "test");
    assert_eq!(root_aggregate(&plan).class, AggregateClass::Distributive);
}

#[test]
fn avg_is_algebraic() {
    let src = r#"
        fn test(q: QueryArray<i32>) -> f64 {
            q.avg()
        }
    "#;
    let plan = plan_for_function(src, "test");
    assert_eq!(root_aggregate(&plan).class, AggregateClass::Algebraic);
}

#[test]
fn min_is_distributive() {
    let src = r#"
        fn test(q: QueryArray<i32>) -> Option<i32> {
            q.min()
        }
    "#;
    let plan = plan_for_function(src, "test");
    assert_eq!(root_aggregate(&plan).class, AggregateClass::Distributive);
}

#[test]
fn max_is_distributive() {
    let src = r#"
        fn test(q: QueryArray<i32>) -> Option<i32> {
            q.max()
        }
    "#;
    let plan = plan_for_function(src, "test");
    assert_eq!(root_aggregate(&plan).class, AggregateClass::Distributive);
}
