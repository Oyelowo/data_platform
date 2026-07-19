//! Tests for QIR scalar expressions.

use yelang_qir::expr::{
    AggregateCall, AggregateClass, Direction, QBinaryOp, QExpr, QExprId, QLit, WindowFunc,
};
use yelang_qir::ids::BinderId;
use yelang_ty::ty::TyId;

fn ty() -> TyId {
    TyId::new(1)
}

#[test]
fn qexpr_literal_round_trips_ty() {
    let lit = QExpr::Lit(QLit::Int(42), ty());
    assert_eq!(lit.ty(), ty());
}

#[test]
fn qexpr_binary_has_correct_ty() {
    let left = QExprId(0);
    let right = QExprId(1);
    let bin = QExpr::Binary(QBinaryOp::Add, left, right, ty());
    assert_eq!(bin.ty(), ty());
}

#[test]
fn aggregate_class_is_hashable() {
    use std::collections::HashSet;
    let mut set = HashSet::new();
    set.insert(AggregateClass::Distributive);
    set.insert(AggregateClass::Algebraic);
    set.insert(AggregateClass::Holistic);
    assert_eq!(set.len(), 3);
}

#[test]
fn aggregate_call_struct_builds() {
    let call = AggregateCall {
        agg_def: yelang_arena::DefId::new(1),
        impl_def: yelang_arena::DefId::new(2),
        class: AggregateClass::Distributive,
        input: QExprId(0),
        per_row: QExprId(1),
        init: QExprId(2),
        step: QExprId(3),
        merge: QExprId(4),
        finish: QExprId(5),
        config: QExprId(6),
        acc_ty: ty(),
        out_ty: ty(),
    };
    assert_eq!(call.class, AggregateClass::Distributive);
}

#[test]
fn direction_and_window_func_are_copy() {
    let d = Direction::Asc;
    let _d2 = d;
    assert_eq!(d, Direction::Asc);

    let w = WindowFunc::RowNumber;
    let _w2 = w;
    assert_eq!(w, WindowFunc::RowNumber);
}

#[test]
fn pattern_binding_builds() {
    let pat = yelang_qir::expr::Pattern::Bind(BinderId(0), ty());
    match pat {
        yelang_qir::expr::Pattern::Bind(b, t) => {
            assert_eq!(b, BinderId(0));
            assert_eq!(t, ty());
        }
        _ => panic!("expected bind pattern"),
    }
}
