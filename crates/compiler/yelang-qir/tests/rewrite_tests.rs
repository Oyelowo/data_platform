//! Tests for logical rewrites.

use yelang_qir::expr::{QBinaryOp, QExpr, QLit};
use yelang_qir::ids::BinderId;
use yelang_qir::logical::operator::{LirOp, ScanSource};
use yelang_qir::logical::plan::LogicalPlan;
use yelang_qir::rewrite::{
    MergeMapsPass, NormalizePass, PushFilterPass, SimplifyPass, apply_rewrites, pass::RewritePass,
};
use yelang_ty::ty::TyId;

fn ty() -> TyId {
    TyId::new(1)
}

fn int_lit(plan: &mut LogicalPlan, v: i128) -> yelang_qir::ids::QExprId {
    plan.alloc_expr(QExpr::Lit(QLit::Int(v), ty()))
}

fn col(plan: &mut LogicalPlan, b: BinderId) -> yelang_qir::ids::QExprId {
    plan.alloc_expr(QExpr::Column(b, ty()))
}

fn binary(plan: &mut LogicalPlan, op: QBinaryOp, l: yelang_qir::ids::QExprId, r: yelang_qir::ids::QExprId) -> yelang_qir::ids::QExprId {
    plan.alloc_expr(QExpr::Binary(op, l, r, ty()))
}

fn closure(plan: &mut LogicalPlan, param: BinderId, body: yelang_qir::ids::QExprId) -> yelang_qir::ids::QExprId {
    plan.alloc_expr(QExpr::Closure {
        params: vec![param],
        body,
        captures: vec![],
        ty: ty(),
    })
}

fn make_scan(plan: &mut LogicalPlan) -> yelang_qir::ids::LirId {
    let source_expr = plan.alloc_expr(QExpr::Lit(QLit::Int(0), ty()));
    plan.scan(ScanSource::Expr(source_expr), ty())
}

#[test]
fn normalize_pass_creates_root_for_empty_plan() {
    let mut plan = LogicalPlan::empty();
    assert!(NormalizePass.run(&mut plan).unwrap());
    assert!(plan.root.is_some());
    // A second run has nothing to do.
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

#[test]
fn normalize_elides_identity_map() {
    let mut plan = LogicalPlan::empty();
    let scan = make_scan(&mut plan);
    let b = plan.fresh_binder();
    let c = col(&mut plan, b);
    let proj = closure(&mut plan, b, c);
    let map = plan.map(scan, proj, ty());
    plan.set_root(map);

    assert!(NormalizePass.run(&mut plan).unwrap());
    assert_eq!(plan.root, Some(scan));
}

#[test]
fn normalize_elides_true_filter() {
    let mut plan = LogicalPlan::empty();
    let scan = make_scan(&mut plan);
    let pred = plan.alloc_expr(QExpr::Lit(QLit::Bool(true), ty()));
    let filter = plan.filter(scan, pred, ty());
    plan.set_root(filter);

    assert!(NormalizePass.run(&mut plan).unwrap());
    assert_eq!(plan.root, Some(scan));
}

#[test]
fn simplify_folds_constant_arithmetic() {
    let mut plan = LogicalPlan::empty();
    let scan = make_scan(&mut plan);
    let l = int_lit(&mut plan, 3);
    let r = int_lit(&mut plan, 4);
    let pred = binary(&mut plan, QBinaryOp::Add, l, r);
    let filter = plan.filter(scan, pred, ty());
    plan.set_root(filter);

    assert!(SimplifyPass.run(&mut plan).unwrap());
    let root = plan.root.unwrap();
    let pred2 = match plan.operator(root) {
        LirOp::Filter { predicate, .. } => *predicate,
        other => panic!("expected Filter, got {:?}", other),
    };
    assert_eq!(plan.expr(pred2), &QExpr::Lit(QLit::Int(7), ty()));
}

#[test]
fn merge_maps_fuses_adjacent_maps() {
    let mut plan = LogicalPlan::empty();
    let scan = make_scan(&mut plan);

    // Map 1: x -> x + 1
    let x = plan.fresh_binder();
    let one = int_lit(&mut plan, 1);
    let cx = col(&mut plan, x);
    let map1_body = binary(&mut plan, QBinaryOp::Add, cx, one);
    let proj1 = closure(&mut plan, x, map1_body);
    let map1 = plan.map(scan, proj1, ty());

    // Map 2: y -> y * 2
    let y = plan.fresh_binder();
    let two = int_lit(&mut plan, 2);
    let cy = col(&mut plan, y);
    let map2_body = binary(&mut plan, QBinaryOp::Mul, cy, two);
    let proj2 = closure(&mut plan, y, map2_body);
    let map2 = plan.map(map1, proj2, ty());
    plan.set_root(map2);

    assert!(MergeMapsPass.run(&mut plan).unwrap());

    let root = plan.root.unwrap();
    match plan.operator(root) {
        LirOp::Map { input, projection } => {
            assert_eq!(*input, scan);
            // The fused projection should be a closure whose body is (param + 1) * 2.
            let body = match plan.expr(*projection) {
                QExpr::Closure { params, body, .. } => {
                    assert_eq!(params.len(), 1);
                    *body
                }
                other => panic!("expected closure projection, got {:?}", other),
            };
            match plan.expr(body) {
                QExpr::Binary(QBinaryOp::Mul, lhs, rhs, _) => {
                    assert_eq!(plan.expr(*rhs), &QExpr::Lit(QLit::Int(2), ty()));
                    match plan.expr(*lhs) {
                        QExpr::Binary(QBinaryOp::Add, ll, rr, _) => {
                            assert_eq!(plan.expr(*rr), &QExpr::Lit(QLit::Int(1), ty()));
                            assert!(matches!(plan.expr(*ll), QExpr::Column(_, _)));
                        }
                        other => panic!("expected Add, got {:?}", other),
                    }
                }
                other => panic!("expected Mul, got {:?}", other),
            }
        }
        other => panic!("expected Map, got {:?}", other),
    }
}

#[test]
fn push_filter_through_map_substitutes_predicate() {
    let mut plan = LogicalPlan::empty();
    let scan = make_scan(&mut plan);

    // Map: x -> x + 1
    let x = plan.fresh_binder();
    let one = int_lit(&mut plan, 1);
    let cx = col(&mut plan, x);
    let map_body = binary(&mut plan, QBinaryOp::Add, cx, one);
    let proj = closure(&mut plan, x, map_body);
    let map = plan.map(scan, proj, ty());

    // Filter: |y| y > 5
    let y = plan.fresh_binder();
    let five = int_lit(&mut plan, 5);
    let cy = col(&mut plan, y);
    let pred_body = binary(&mut plan, QBinaryOp::Gt, cy, five);
    let pred = closure(&mut plan, y, pred_body);
    let filter = plan.filter(map, pred, ty());
    plan.set_root(filter);

    assert!(PushFilterPass.run(&mut plan).unwrap());

    let root = plan.root.unwrap();
    match plan.operator(root) {
        LirOp::Map { input, projection } => {
            assert!(matches!(plan.expr(*projection), QExpr::Closure { .. }));
            match plan.operator(*input) {
                LirOp::Filter { input: scan2, predicate } => {
                    assert_eq!(*scan2, scan);
                    // The pushed predicate body should be (x + 1) > 5.
                    match plan.expr(*predicate) {
                        QExpr::Binary(QBinaryOp::Gt, lhs, rhs, _) => {
                            assert_eq!(plan.expr(*rhs), &QExpr::Lit(QLit::Int(5), ty()));
                            match plan.expr(*lhs) {
                                QExpr::Binary(QBinaryOp::Add, ll, rr, _) => {
                                    assert_eq!(plan.expr(*rr), &QExpr::Lit(QLit::Int(1), ty()));
                                    assert!(matches!(plan.expr(*ll), QExpr::Column(_, _)));
                                }
                                other => panic!("expected Add, got {:?}", other),
                            }
                        }
                        other => panic!("expected Gt, got {:?}", other),
                    }
                }
                other => panic!("expected Filter below Map, got {:?}", other),
            }
        }
        other => panic!("expected Map at root, got {:?}", other),
    }
}

#[test]
fn apply_rewrites_merges_and_pushes_in_fixpoint() {
    // Filter(Map(Map(scan, x -> x + 1), y -> y * 2), z > 10)
    // Should end as a single Map over a Filter over scan.
    let mut plan = LogicalPlan::empty();
    let scan = make_scan(&mut plan);

    let x = plan.fresh_binder();
    let one = int_lit(&mut plan, 1);
    let cx = col(&mut plan, x);
    let add_body = binary(&mut plan, QBinaryOp::Add, cx, one);
    let p1 = closure(&mut plan, x, add_body);
    let map1 = plan.map(scan, p1, ty());

    let y = plan.fresh_binder();
    let two = int_lit(&mut plan, 2);
    let cy = col(&mut plan, y);
    let mul_body = binary(&mut plan, QBinaryOp::Mul, cy, two);
    let p2 = closure(&mut plan, y, mul_body);
    let map2 = plan.map(map1, p2, ty());

    let z = plan.fresh_binder();
    let ten = int_lit(&mut plan, 10);
    let cz = col(&mut plan, z);
    let pred = binary(&mut plan, QBinaryOp::Gt, cz, ten);
    let filter = plan.filter(map2, pred, ty());
    plan.set_root(filter);

    apply_rewrites(&mut plan).unwrap();

    let root = plan.root.unwrap();
    match plan.operator(root) {
        LirOp::Map { input, .. } => match plan.operator(*input) {
            LirOp::Filter { input: scan2, .. } => {
                assert_eq!(*scan2, scan);
            }
            other => panic!("expected Filter below final Map, got {:?}", other),
        },
        other => panic!("expected Map at root, got {:?}", other),
    }
}
