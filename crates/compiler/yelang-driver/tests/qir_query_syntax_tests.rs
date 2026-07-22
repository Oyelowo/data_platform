//! Tests for lowering `select ... from ...` query syntax through the THIR →
//! LIR extractor.

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
fn select_identity_from_param_is_a_scan() {
    // Identity projection is elided by the optimizer, so the pipeline collapses
    // to a single scan over the source collection.
    let src = r#"
        fn test(users: [i32]) -> i32 {
            select users[*].age from users@u
        }
    "#;
    let plan = plan_for_function(src, "test");
    let root = root_op(&plan);
    let LirOp::Scan { .. } = root else {
        panic!("expected Scan root after identity-elision, got {:?}", root);
    };
}

#[test]
fn select_with_where_clause() {
    let src = r#"
        fn test(users: [i32]) -> i32 {
            select users@u[*].{
                age: age + 10
            } 
            from users@u where u > 2
        }
    "#;
    let plan = plan_for_function(src, "test");
    let root = root_op(&plan);
    let LirOp::Map { .. } = root else {
        panic!("expected Map root, got {:?}", root);
    };
    let filter = child_input(&plan, root);
    let LirOp::Filter { .. } = filter else {
        panic!("expected Filter under Map, got {:?}", filter);
    };
    let scan = child_input(&plan, filter);
    let LirOp::Scan { .. } = scan else {
        panic!("expected Scan under Filter, got {:?}", scan);
    };
}

#[test]
fn select_order_by_and_range() {
    let src = r#"
        fn test(users: [i32]) -> i32 {
            select users@u[*].age from users@u order by u.age asc range 1..3
        }
    "#;
    let plan = plan_for_function(src, "test");
    let root = root_op(&plan);
    // Identity projection is elided, so the root is the range/slice operator.
    let LirOp::Slice { limit, .. } = root else {
        panic!("expected Slice root, got {:?}", root);
    };
    assert!(limit.is_some(), "range should produce a limit");
    let order = child_input(&plan, root);
    let LirOp::OrderBy { .. } = order else {
        panic!("expected OrderBy under Slice, got {:?}", order);
    };
}

#[test]
fn select_from_local_array_let_binding() {
    let src = r#"
        fn test() -> i32 {
            let users = [1, 2, 3, 4, 5];
            let users_age = select users@u[*].{
                age: u.age + 10
            }
            from users@u where u.age > 2;
            
            users_age.map(|u| u.age)
        }
    "#;
    let plan = plan_for_function(src, "test");
    let root = root_op(&plan);
    let LirOp::Map { .. } = root else {
        panic!("expected Map root, got {:?}", root);
    };
    let filter = child_input(&plan, root);
    let LirOp::Filter { .. } = filter else {
        panic!("expected Filter under Map, got {:?}", filter);
    };
}
