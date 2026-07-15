use crate::interner::Interner;
use crate::primitive::{IntTy, UintTy};
use crate::ty::{FloatVid, InferTy, IntVid, TyKind, TyVid};

#[test]
fn ty_interning_preserves_equality() {
    let interner = Interner::new();
    let a = interner.mk_ty(TyKind::Bool);
    let b = interner.mk_ty(TyKind::Bool);
    assert_eq!(a, b);
    assert_eq!(a.as_ptr(), b.as_ptr());
}

#[test]
fn all_primitive_types_intern() {
    let interner = Interner::new();
    let primitives = [
        interner.mk_ty(TyKind::Bool),
        interner.mk_ty(TyKind::Char),
        interner.mk_ty(TyKind::Int(IntTy::I8)),
        interner.mk_ty(TyKind::Int(IntTy::I16)),
        interner.mk_ty(TyKind::Int(IntTy::I32)),
        interner.mk_ty(TyKind::Int(IntTy::I64)),
        interner.mk_ty(TyKind::Int(IntTy::I128)),
        interner.mk_ty(TyKind::Int(IntTy::Isize)),
        interner.mk_ty(TyKind::Uint(UintTy::U8)),
        interner.mk_ty(TyKind::Uint(UintTy::U16)),
        interner.mk_ty(TyKind::Uint(UintTy::U32)),
        interner.mk_ty(TyKind::Uint(UintTy::U64)),
        interner.mk_ty(TyKind::Uint(UintTy::U128)),
        interner.mk_ty(TyKind::Uint(UintTy::Usize)),
        interner.mk_ty(TyKind::Float(crate::primitive::FloatTy::F32)),
        interner.mk_ty(TyKind::Float(crate::primitive::FloatTy::F64)),
        interner.mk_ty(TyKind::Never),
    ];

    // All should be distinct
    for i in 0..primitives.len() {
        for j in (i + 1)..primitives.len() {
            assert_ne!(
                primitives[i], primitives[j],
                "types at {} and {} should differ",
                i, j
            );
        }
    }
}

#[test]
fn infer_ty_distinct_ids() {
    let v1 = InferTy::TyVar(TyVid(0));
    let v2 = InferTy::TyVar(TyVid(1));
    let v3 = InferTy::IntVar(IntVid(0));
    let v4 = InferTy::FloatVar(FloatVid(0));
    assert_ne!(v1, v2);
    assert_ne!(v1, v3);
    assert_ne!(v1, v4);
    assert_ne!(v3, v4);
}
