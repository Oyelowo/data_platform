use yelang_ty::interner::Interner;
use yelang_ty::primitive::IntTy;
use yelang_ty::ty::Ty;

use crate::builtin::is_sized;

#[test]
fn sized_primitive_types() {
    let interner = Interner::new();
    assert!(is_sized(interner.mk_ty(Ty::Bool), &interner));
    assert!(is_sized(interner.mk_ty(Ty::Int(IntTy::I32)), &interner));
    assert!(is_sized(interner.mk_ty(Ty::Never), &interner));
}

#[test]
fn sized_tuple() {
    let interner = Interner::new();
    let tuple = interner.mk_ty(Ty::Tuple(interner.mk_generic_args(&[
        yelang_ty::generic::GenericArg::Type(interner.mk_ty(Ty::Int(IntTy::I32))),
        yelang_ty::generic::GenericArg::Type(interner.mk_ty(Ty::Bool)),
    ])));
    assert!(is_sized(tuple, &interner));
}

#[test]
fn unsized_slice() {
    let interner = Interner::new();
    let slice = interner.mk_ty(Ty::Slice(interner.mk_ty(Ty::Int(IntTy::I32))));
    assert!(!is_sized(slice, &interner));
}
