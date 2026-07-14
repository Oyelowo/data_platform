use yelang_ty::interner::Interner;
use yelang_ty::primitive::IntTy;
use yelang_ty::ty::TyKind;

use crate::builtin::is_sized;

#[test]
fn sized_primitive_types() {
    let interner = Interner::new();
    assert!(is_sized(interner.mk_ty(TyKind::Bool).kind()));
    assert!(is_sized(interner.mk_ty(TyKind::Int(IntTy::I32)).kind()));
    assert!(is_sized(interner.mk_ty(TyKind::Never).kind()));
}

#[test]
fn sized_tuple() {
    let interner = Interner::new();
    let tuple = interner.mk_ty(TyKind::Tuple(interner.mk_generic_args(&[
        yelang_ty::generic::GenericArg::Type(interner.mk_ty(TyKind::Int(IntTy::I32))),
        yelang_ty::generic::GenericArg::Type(interner.mk_ty(TyKind::Bool)),
    ])));
    assert!(is_sized(tuple.kind()));
}

#[test]
fn unsized_slice() {
    let interner = Interner::new();
    let slice = interner.mk_ty(TyKind::Slice(interner.mk_ty(TyKind::Int(IntTy::I32))));
    assert!(!is_sized(slice.kind()));
}
