//! Tests for logical rewrites.

use yelang_interner::Symbol;
use yelang_qir::expr::{QBinaryOp, QExpr, QLit};
use yelang_qir::ids::BinderId;
use yelang_qir::logical::operator::{ConstructKind, LirOp, ScanSource};
use yelang_qir::logical::plan::LogicalPlan;
use yelang_qir::rewrite::{
    DecorrelatePass, MergeMapsPass, NormalizePass, PredicatePushdownPass, ProjectionPushdownPass,
    PushFilterPass, PushProjectPass, SimplifyPass, apply_rewrites, pass::RewritePass,
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

fn field_expr(
    plan: &mut LogicalPlan,
    base: yelang_qir::ids::QExprId,
    name: Symbol,
) -> yelang_qir::ids::QExprId {
    plan.alloc_expr(QExpr::Field(base, name, ty()))
}

fn record_expr(
    plan: &mut LogicalPlan,
    fields: Vec<(Symbol, yelang_qir::ids::QExprId)>,
) -> yelang_qir::ids::QExprId {
    plan.alloc_expr(QExpr::Record(fields, ty()))
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

#[test]
fn projection_pushdown_trims_record_map() {
    let mut plan = LogicalPlan::empty();
    let scan = make_scan(&mut plan);

    let x = plan.fresh_binder();
    let cx = col(&mut plan, x);
    let a_sym = Symbol::from(1);
    let b_sym = Symbol::from(2);
    let fa = field_expr(&mut plan, cx, a_sym);
    let fb = field_expr(&mut plan, cx, b_sym);
    let body = record_expr(&mut plan, vec![(a_sym, fa), (b_sym, fb)]);
    let proj = closure(&mut plan, x, body);
    let map = plan.map(scan, proj, ty());

    // A downstream map selects only field `a`, so the source map can drop `b`.
    let y = plan.fresh_binder();
    let cy = col(&mut plan, y);
    let select_a = field_expr(&mut plan, cy, a_sym);
    let selector = closure(&mut plan, y, select_a);
    let selector_map = plan.map(map, selector, ty());
    plan.set_root(selector_map);

    assert!(ProjectionPushdownPass.run(&mut plan).unwrap());

    let root = plan.root.unwrap();
    let source_map = match plan.operator(root) {
        LirOp::Map { input, .. } => *input,
        other => panic!("expected Map at root, got {:?}", other),
    };
    let projection = match plan.operator(source_map) {
        LirOp::Map { projection, .. } => *projection,
        other => panic!("expected source Map, got {:?}", other),
    };
    let body = match plan.expr(projection) {
        QExpr::Closure { body, .. } => *body,
        other => panic!("expected closure projection, got {:?}", other),
    };
    match plan.expr(body) {
        QExpr::Record(fields, _) => {
            assert_eq!(fields.len(), 1);
            assert_eq!(fields[0].0, a_sym);
        }
        other => panic!("expected Record, got {:?}", other),
    }
}

#[test]
fn projection_pushdown_trims_construct() {
    let mut plan = LogicalPlan::empty();
    let a = make_scan(&mut plan);
    let b = make_scan(&mut plan);

    let a_sym = Symbol::from(1);
    let b_sym = Symbol::from(2);
    let inner = plan.construct(ConstructKind::Record, vec![(a_sym, a), (b_sym, b)], ty());

    // A downstream map selects only field `a` from the inner construct.
    let y = plan.fresh_binder();
    let cy = col(&mut plan, y);
    let select_a = field_expr(&mut plan, cy, a_sym);
    let selector = closure(&mut plan, y, select_a);
    let selector_map = plan.map(inner, selector, ty());
    plan.set_root(selector_map);

    assert!(ProjectionPushdownPass.run(&mut plan).unwrap());

    let root = plan.root.unwrap();
    let inner_id = match plan.operator(root) {
        LirOp::Map { input, .. } => *input,
        other => panic!("expected Map at root, got {:?}", other),
    };
    match plan.operator(inner_id) {
        LirOp::Construct { fields, .. } => {
            assert_eq!(fields.len(), 1);
            assert_eq!(fields[0].0, a_sym);
        }
        other => panic!("expected inner Construct, got {:?}", other),
    }
}

#[test]
fn push_project_through_slice() {
    let mut plan = LogicalPlan::empty();
    let scan = make_scan(&mut plan);

    let x = plan.fresh_binder();
    let one = int_lit(&mut plan, 1);
    let cx = col(&mut plan, x);
    let add_body = binary(&mut plan, QBinaryOp::Add, cx, one);
    let proj = closure(&mut plan, x, add_body);
    let map = plan.map(scan, proj, ty());

    let offset = int_lit(&mut plan, 0);
    let limit = int_lit(&mut plan, 10);
    let slice = plan.slice_unordered(map, offset, Some(limit), ty());
    plan.set_root(slice);

    assert!(PushProjectPass.run(&mut plan).unwrap());

    let root = plan.root.unwrap();
    match plan.operator(root) {
        LirOp::Map { input, .. } => match plan.operator(*input) {
            LirOp::Slice { input: scan2, .. } => {
                assert_eq!(*scan2, scan);
            }
            other => panic!("expected Slice below Map, got {:?}", other),
        },
        other => panic!("expected Map at root, got {:?}", other),
    }
}

#[test]
fn predicate_pushdown_merges_nested_filters() {
    let mut plan = LogicalPlan::empty();
    let scan = make_scan(&mut plan);

    let x = plan.fresh_binder();
    let five = int_lit(&mut plan, 5);
    let cx = col(&mut plan, x);
    let p1_body = binary(&mut plan, QBinaryOp::Gt, cx, five);
    let p1 = closure(&mut plan, x, p1_body);
    let f1 = plan.filter(scan, p1, ty());

    let y = plan.fresh_binder();
    let ten = int_lit(&mut plan, 10);
    let cy = col(&mut plan, y);
    let p2_body = binary(&mut plan, QBinaryOp::Lt, cy, ten);
    let p2 = closure(&mut plan, y, p2_body);
    let f2 = plan.filter(f1, p2, ty());
    plan.set_root(f2);

    assert!(PredicatePushdownPass.run(&mut plan).unwrap());

    let root = plan.root.unwrap();
    match plan.operator(root) {
        LirOp::Filter { input, predicate } => {
            assert_eq!(*input, scan);
            // The merged predicate should be a closure whose body is (x > 5) && (x < 10).
            let body = match plan.expr(*predicate) {
                QExpr::Closure { params, body, .. } => {
                    assert_eq!(params.len(), 1);
                    *body
                }
                other => panic!("expected closure predicate, got {:?}", other),
            };
            match plan.expr(body) {
                QExpr::Binary(QBinaryOp::And, lhs, rhs, _) => {
                    assert!(matches!(plan.expr(*lhs), QExpr::Binary(QBinaryOp::Gt, _, _, _)));
                    assert!(matches!(plan.expr(*rhs), QExpr::Binary(QBinaryOp::Lt, _, _, _)));
                }
                other => panic!("expected And, got {:?}", other),
            }
        }
        other => panic!("expected Filter at root, got {:?}", other),
    }
}

#[test]
fn predicate_pushdown_cross_join_to_inner_join() {
    use yelang_qir::logical::operator::JoinKind;

    let mut plan = LogicalPlan::empty();
    let left = make_scan(&mut plan);
    let right = make_scan(&mut plan);
    let cross = plan.join(JoinKind::Cross, left, right, None, ty());

    let z = plan.fresh_binder();
    let five = int_lit(&mut plan, 5);
    let cz = col(&mut plan, z);
    let pred_body = binary(&mut plan, QBinaryOp::Gt, cz, five);
    let pred = closure(&mut plan, z, pred_body);
    let filter = plan.filter(cross, pred, ty());
    plan.set_root(filter);

    assert!(PredicatePushdownPass.run(&mut plan).unwrap());

    let root = plan.root.unwrap();
    match plan.operator(root) {
        LirOp::Join {
            kind,
            left: l,
            right: r,
            predicate,
        } => {
            assert_eq!(*kind, JoinKind::Inner);
            assert_eq!(*l, left);
            assert_eq!(*r, right);
            assert!(predicate.is_some());
        }
        other => panic!("expected Join at root, got {:?}", other),
    }
}

#[test]
fn predicate_pushdown_merges_inner_join_predicate() {
    use yelang_qir::logical::operator::JoinKind;

    let mut plan = LogicalPlan::empty();
    let left = make_scan(&mut plan);
    let right = make_scan(&mut plan);

    let jp = plan.alloc_expr(QExpr::Lit(QLit::Bool(true), ty()));
    let inner = plan.join(JoinKind::Inner, left, right, Some(jp), ty());

    let z = plan.fresh_binder();
    let five = int_lit(&mut plan, 5);
    let cz = col(&mut plan, z);
    let pred_body = binary(&mut plan, QBinaryOp::Gt, cz, five);
    let pred = closure(&mut plan, z, pred_body);
    let filter = plan.filter(inner, pred, ty());
    plan.set_root(filter);

    assert!(PredicatePushdownPass.run(&mut plan).unwrap());

    let root = plan.root.unwrap();
    match plan.operator(root) {
        LirOp::Join {
            kind,
            predicate,
            ..
        } => {
            assert_eq!(*kind, JoinKind::Inner);
            let predicate = predicate.expect("expected merged predicate");
            match plan.expr(predicate) {
                QExpr::Closure { body, .. } => match plan.expr(*body) {
                    QExpr::Binary(QBinaryOp::And, _, _, _) => {}
                    other => panic!("expected And body, got {:?}", other),
                },
                other => panic!("expected closure predicate, got {:?}", other),
            }
        }
        other => panic!("expected Join at root, got {:?}", other),
    }
}


#[test]
fn decorrelate_uncorrelated_to_cross_join() {
    use yelang_qir::logical::operator::JoinKind;

    let mut plan = LogicalPlan::empty();
    let outer_scan = make_scan(&mut plan);
    plan.props[outer_scan].output_binder = Some(BinderId(100));

    let inner_scan = make_scan(&mut plan);
    plan.props[inner_scan].output_binder = Some(BinderId(200));
    let x = plan.fresh_binder();
    let one = int_lit(&mut plan, 1);
    let cx = col(&mut plan, x);
    let add_body = binary(&mut plan, QBinaryOp::Add, cx, one);
    let proj = closure(&mut plan, x, add_body);
    let inner_map = plan.map(inner_scan, proj, ty());
    plan.props[inner_map].output_binder = Some(x);

    let dj = plan.dependent_join(outer_scan, inner_map, None, ty());
    plan.set_root(dj);

    assert!(DecorrelatePass.run(&mut plan).unwrap());

    let root = plan.root.unwrap();
    match plan.operator(root) {
        LirOp::Join {
            kind,
            left,
            right,
            predicate,
        } => {
            assert_eq!(*kind, JoinKind::Cross);
            assert_eq!(*left, outer_scan);
            assert_eq!(*right, inner_map);
            assert!(predicate.is_none());
        }
        other => panic!("expected Join at root, got {:?}", other),
    }
}

#[test]
fn decorrelate_uncorrelated_filter_to_cross_join() {
    use yelang_qir::logical::operator::JoinKind;

    let mut plan = LogicalPlan::empty();
    let outer_scan = make_scan(&mut plan);
    plan.props[outer_scan].output_binder = Some(BinderId(100));

    let inner_scan = make_scan(&mut plan);
    plan.props[inner_scan].output_binder = Some(BinderId(200));

    // Filter(|i| i > 0) over inner scan — does not reference outer binder.
    let i = plan.fresh_binder();
    let zero = int_lit(&mut plan, 0);
    let ci = col(&mut plan, i);
    let pred_body = binary(&mut plan, QBinaryOp::Gt, ci, zero);
    let pred = closure(&mut plan, i, pred_body);
    let inner_filter = plan.filter(inner_scan, pred, ty());
    plan.props[inner_filter].output_binder = Some(i);

    let dj = plan.dependent_join(outer_scan, inner_filter, None, ty());
    plan.set_root(dj);

    assert!(DecorrelatePass.run(&mut plan).unwrap());

    let root = plan.root.unwrap();
    match plan.operator(root) {
        LirOp::Join {
            kind,
            left,
            right,
            predicate,
        } => {
            assert_eq!(*kind, JoinKind::Cross);
            assert_eq!(*left, outer_scan);
            assert_eq!(*right, inner_filter);
            assert!(predicate.is_none());
        }
        other => panic!("expected Join at root, got {:?}", other),
    }
}

#[test]
fn decorrelate_pushes_through_filter_over_correlated_scan() {
    let mut plan = LogicalPlan::empty();
    let outer_scan = make_scan(&mut plan);
    let outer_binder = plan.fresh_binder();
    plan.props[outer_scan].output_binder = Some(outer_binder);

    // Inner scan source references the outer binder (simulates a correlated source).
    let inner_scan_source = col(&mut plan, outer_binder);
    let inner_scan = plan.scan(ScanSource::Expr(inner_scan_source), ty());
    let inner_binder = plan.fresh_binder();
    plan.props[inner_scan].output_binder = Some(inner_binder);

    // Filter(|i| i > 0) does not itself reference the outer binder.
    let zero = int_lit(&mut plan, 0);
    let ci = col(&mut plan, inner_binder);
    let pred_body = binary(&mut plan, QBinaryOp::Gt, ci, zero);
    let pred = closure(&mut plan, inner_binder, pred_body);
    let inner_filter = plan.filter(inner_scan, pred, ty());
    plan.props[inner_filter].output_binder = Some(inner_binder);

    let dj = plan.dependent_join(outer_scan, inner_filter, None, ty());
    plan.set_root(dj);

    assert!(DecorrelatePass.run(&mut plan).unwrap());

    let root = plan.root.unwrap();
    match plan.operator(root) {
        LirOp::Filter { input, .. } => {
            assert!(matches!(plan.operator(*input), LirOp::DependentJoin { .. }));
        }
        other => panic!("expected Filter at root, got {:?}", other),
    }
}

#[test]
fn decorrelate_keeps_correlated_map() {
    let mut plan = LogicalPlan::empty();
    let outer_scan = make_scan(&mut plan);
    let outer_binder = plan.fresh_binder();
    plan.props[outer_scan].output_binder = Some(outer_binder);
    let outer_map_body = col(&mut plan, outer_binder);
    let outer_proj = closure(&mut plan, outer_binder, outer_map_body);
    let outer_map = plan.map(outer_scan, outer_proj, ty());
    plan.props[outer_map].output_binder = Some(outer_binder);

    let inner_scan = make_scan(&mut plan);
    plan.props[inner_scan].output_binder = Some(BinderId(300));

    // Map(|i| i + outer_binder) references an outer binder.
    let i = plan.fresh_binder();
    let c_outer = col(&mut plan, outer_binder);
    let ci = col(&mut plan, i);
    let add_body = binary(&mut plan, QBinaryOp::Add, ci, c_outer);
    let proj = closure(&mut plan, i, add_body);
    let inner_map = plan.map(inner_scan, proj, ty());
    plan.props[inner_map].output_binder = Some(i);

    let dj = plan.dependent_join(outer_map, inner_map, None, ty());
    plan.set_root(dj);

    // The top map references outer, so DJ cannot be pushed or eliminated.
    assert!(!DecorrelatePass.run(&mut plan).unwrap());
    assert!(matches!(plan.operator(plan.root.unwrap()), LirOp::DependentJoin { .. }));
}

#[test]
fn unnest_uncorrelated_scalar_subplan_to_cross_join() {
    use yelang_qir::logical::operator::JoinKind;
    use yelang_qir::logical::props::CardinalityClass;
    use yelang_qir::rewrite::UnnestSubqueriesPass;

    let mut plan = LogicalPlan::empty();

    // Outer collection: [1, 2, 3]
    let outer_exprs = vec![int_lit(&mut plan, 1), int_lit(&mut plan, 2), int_lit(&mut plan, 3)];
    let outer_values = plan.values(outer_exprs, ty());
    let outer_binder = plan.fresh_binder();
    plan.props[outer_values].output_binder = Some(outer_binder);

    // Scalar subplan: a single value [30]
    let sub_expr_30 = int_lit(&mut plan, 30);
    let sub_values = plan.values(vec![sub_expr_30], ty());
    plan.props[sub_values].cardinality = CardinalityClass::One;
    let sub_binder = plan.fresh_binder();
    plan.props[sub_values].output_binder = Some(sub_binder);

    // Predicate: outer > subplan
    let sub_expr = plan.alloc_expr(QExpr::Subplan(sub_values, ty()));
    let c_outer = col(&mut plan, outer_binder);
    let pred_body = binary(&mut plan, QBinaryOp::Gt, c_outer, sub_expr);
    let pred = closure(&mut plan, outer_binder, pred_body);
    let filter = plan.filter(outer_values, pred, ty());
    plan.props[filter].output_binder = Some(outer_binder);
    plan.set_root(filter);

    assert!(UnnestSubqueriesPass.run(&mut plan).unwrap());

    let root = plan.root.unwrap();
    let filter_op = plan.operator(root).clone();
    let LirOp::Filter { input, predicate } = filter_op else {
        panic!("expected Filter at root");
    };

    // The filter input should now be a cross join of the outer collection and
    // the scalar subplan.
    match plan.operator(input) {
        LirOp::Join {
            kind,
            left,
            right,
            predicate: None,
        } => {
            assert_eq!(*kind, JoinKind::Cross);
            assert_eq!(*left, outer_values);
            assert_eq!(*right, sub_values);
        }
        other => panic!("expected cross Join below Filter, got {:?}", other),
    }

    // The predicate should now access the left/right fields of the joined row.
    let body = match plan.expr(predicate) {
        QExpr::Closure { body, .. } => *body,
        other => panic!("expected closure predicate, got {:?}", other),
    };
    match plan.expr(body) {
        QExpr::Binary(QBinaryOp::Gt, l, r, _) => {
            match plan.expr(*l) {
                QExpr::Field(_, sym, _) => assert_eq!(sym.as_usize(), 1),
                other => panic!("expected left field access, got {:?}", other),
            }
            match plan.expr(*r) {
                QExpr::Field(_, sym, _) => assert_eq!(sym.as_usize(), 2),
                other => panic!("expected right field access, got {:?}", other),
            }
        }
        other => panic!("expected Gt body, got {:?}", other),
    }
}
