//! Tests for the physical planner.

use yelang_qir::backend::memory::MemoryBackend;
use yelang_qir::expr::{QBinaryOp, QExpr, QLit};
use yelang_qir::lir::operator::ScanSource;
use yelang_qir::lir::plan::LogicalPlan;
use yelang_qir::pir::operator::PirOp;
use yelang_qir::pir::planner::plan_logical;
use yelang_ty::ty::TyId;

fn ty() -> TyId {
    TyId::new(1)
}

fn int_lit(plan: &mut LogicalPlan, v: i128) -> yelang_qir::ids::QExprId {
    plan.alloc_expr(QExpr::Lit(QLit::Int(v), ty()))
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

#[test]
fn planner_maps_scan_filter_projection() {
    let mut plan = LogicalPlan::empty();
    let source_expr = plan.alloc_expr(QExpr::Lit(QLit::Int(0), ty()));
    let scan = plan.scan(ScanSource::Expr(source_expr), ty());
    plan.props[scan].output_binder = Some(yelang_qir::ids::BinderId(1));

    // filter: x -> x > 5
    let x = plan.fresh_binder();
    let five = int_lit(&mut plan, 5);
    let cx = col(&mut plan, x);
    let pred_body = plan.alloc_expr(QExpr::Binary(QBinaryOp::Gt, cx, five, ty()));
    let pred = closure(&mut plan, x, pred_body);
    let filter = plan.filter(scan, pred, ty());
    plan.props[filter].output_binder = Some(x);

    // map: y -> y + 1
    let y = plan.fresh_binder();
    let one = int_lit(&mut plan, 1);
    let cy = col(&mut plan, y);
    let map_body = plan.alloc_expr(QExpr::Binary(QBinaryOp::Add, cy, one, ty()));
    let proj = closure(&mut plan, y, map_body);
    let map = plan.map(filter, proj, ty());
    plan.set_root(map);

    let physical = plan_logical(&plan, &MemoryBackend::new()).unwrap();
    let root = physical.root.unwrap();

    // Expected: Project(Filter(TableScan))
    match physical.operator(root) {
        PirOp::Project { input, .. } => match physical.operator(*input) {
            PirOp::Filter { input, .. } => {
                assert!(matches!(physical.operator(*input), PirOp::TableScan { .. }));
            }
            other => panic!("expected Filter below Project, got {:?}", other),
        },
        other => panic!("expected Project at root, got {:?}", other),
    }
}

#[test]
fn planner_orders_and_slices() {
    let mut plan = LogicalPlan::empty();
    let source_expr = plan.alloc_expr(QExpr::Lit(QLit::Int(0), ty()));
    let scan = plan.scan(ScanSource::Expr(source_expr), ty());
    plan.props[scan].output_binder = Some(yelang_qir::ids::BinderId(1));

    let x = plan.fresh_binder();
    let key = col(&mut plan, x);
    let order = plan.order_by(scan, vec![yelang_qir::expr::OrderKey { expr: key, dir: yelang_qir::expr::Direction::Asc, nulls: yelang_qir::expr::NullOrdering::Last }], ty());
    plan.props[order].output_binder = Some(x);

    let offset = int_lit(&mut plan, 0);
    let limit = int_lit(&mut plan, 10);
    let slice = plan.slice(order, offset, Some(limit), ty()).unwrap();
    plan.set_root(slice);

    let physical = plan_logical(&plan, &MemoryBackend::new()).unwrap();
    let root = physical.root.unwrap();
    assert!(matches!(physical.operator(root), PirOp::Slice { .. }));
}
