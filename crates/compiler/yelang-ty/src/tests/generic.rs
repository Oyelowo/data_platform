use crate::generic::{GenericArg, GenericParamCount, Substitution};
use crate::interner::Interner;
use crate::primitive::IntTy;
use crate::ty::TyKind;

#[test]
fn substitution_empty() {
    let sub = Substitution::empty();
    assert!(sub.is_empty());
    assert_eq!(sub.len(), 0);
}

#[test]
fn substitution_type_at() {
    let interner = Interner::new();
    let t1 = interner.mk_ty(TyKind::Int(IntTy::I32));
    let t2 = interner.mk_ty(TyKind::Bool);
    let sub = Substitution::from_args(vec![GenericArg::Type(t1), GenericArg::Type(t2)]);
    assert_eq!(sub.type_at(0), t1);
    assert_eq!(sub.type_at(1), t2);
}

#[test]
fn generic_param_count_total() {
    let count = GenericParamCount {
        type_params: 2,
        const_params: 1,
    };
    assert_eq!(count.total(), 3);
}
