use yelang_arena::DefId;
use yelang_interner::Symbol;
use yelang_ty::binder::{BoundTy, BoundTyKind, BoundVar, DebruijnIndex};
use yelang_ty::generic::GenericArg;
use yelang_ty::interner::Interner;
use yelang_ty::predicate::TraitRef;
use yelang_ty::primitive::{FloatTy, IntTy};
use yelang_ty::projection::ProjectionTy;
use yelang_ty::ty::{Const, ConstValue, PlaceholderType, Ty, UniverseIndex};

use crate::context::InferCtxt;
use crate::error::TypeError;

#[test]
fn eq_const_unifies_infer_vars() {
    let interner = Interner::new();
    let mut infcx = InferCtxt::new();
    let t_i32 = interner.mk_ty(Ty::Int(IntTy::I32));
    let a = infcx.new_const_var(&interner, t_i32);
    let b = infcx.new_const_var(&interner, t_i32);
    assert!(infcx.eq_const(&interner, a, b).is_ok());
}

#[test]
fn eq_const_unifies_infer_with_value() {
    let interner = Interner::new();
    let mut infcx = InferCtxt::new();
    let t_i32 = interner.mk_ty(Ty::Int(IntTy::I32));
    let var = infcx.new_const_var(&interner, t_i32);
    let value = interner.mk_const_from_parts(Const::Value(ConstValue::Int(42)), t_i32);
    assert!(infcx.eq_const(&interner, var, value).is_ok());
    // A second variable unified with the same value should also succeed.
    let var2 = infcx.new_const_var(&interner, t_i32);
    assert!(infcx.eq_const(&interner, var2, value).is_ok());
}

#[test]
fn eq_const_rejects_value_mismatch() {
    let interner = Interner::new();
    let mut infcx = InferCtxt::new();
    let t_i32 = interner.mk_ty(Ty::Int(IntTy::I32));
    let a = interner.mk_const_from_parts(Const::Value(ConstValue::Int(1)), t_i32);
    let b = interner.mk_const_from_parts(Const::Value(ConstValue::Int(2)), t_i32);
    match infcx.eq_const(&interner, a, b) {
        Err(TypeError::ConstMismatch { expected, found }) => {
            assert_eq!(expected, a);
            assert_eq!(found, b);
        }
        other => panic!("expected ConstMismatch, got {:?}", other),
    }
}

#[test]
fn eq_int_var_rejects_mismatch() {
    let interner = Interner::new();
    let mut infcx = InferCtxt::new();
    let var = infcx.new_int_var(&interner);
    let t_i32 = interner.mk_ty(Ty::Int(IntTy::I32));
    let t_i64 = interner.mk_ty(Ty::Int(IntTy::I64));
    assert!(infcx.eq(&interner, var, t_i32).is_ok());
    match infcx.eq(&interner, var, t_i64) {
        Err(TypeError::IntMismatch { expected, found }) => {
            assert_eq!(expected, IntTy::I32);
            assert_eq!(found, IntTy::I64);
        }
        other => panic!("expected IntMismatch, got {:?}", other),
    }
}

#[test]
fn eq_float_var_rejects_mismatch() {
    let interner = Interner::new();
    let mut infcx = InferCtxt::new();
    let var = infcx.new_float_var(&interner);
    let t_f32 = interner.mk_ty(Ty::Float(FloatTy::F32));
    let t_f64 = interner.mk_ty(Ty::Float(FloatTy::F64));
    assert!(infcx.eq(&interner, var, t_f32).is_ok());
    match infcx.eq(&interner, var, t_f64) {
        Err(TypeError::FloatMismatch { expected, found }) => {
            assert_eq!(expected, FloatTy::F32);
            assert_eq!(found, FloatTy::F64);
        }
        other => panic!("expected FloatMismatch, got {:?}", other),
    }
}

#[test]
fn eq_placeholder_equal() {
    let interner = Interner::new();
    let mut infcx = InferCtxt::new();
    let p = PlaceholderType {
        universe: UniverseIndex(0),
        name: Symbol::from(1),
    };
    let a = interner.mk_ty(Ty::Placeholder(p));
    let b = interner.mk_ty(Ty::Placeholder(p));
    assert!(infcx.eq(&interner, a, b).is_ok());
}

#[test]
fn eq_placeholder_different_rejects() {
    let interner = Interner::new();
    let mut infcx = InferCtxt::new();
    let a = interner.mk_ty(Ty::Placeholder(PlaceholderType {
        universe: UniverseIndex(0),
        name: Symbol::from(1),
    }));
    let b = interner.mk_ty(Ty::Placeholder(PlaceholderType {
        universe: UniverseIndex(0),
        name: Symbol::from(2),
    }));
    match infcx.eq(&interner, a, b) {
        Err(TypeError::Mismatch { expected, found }) => {
            assert_eq!(expected, a);
            assert_eq!(found, b);
        }
        other => panic!("expected Mismatch, got {:?}", other),
    }
}

#[test]
fn eq_bound_var_equal() {
    let interner = Interner::new();
    let mut infcx = InferCtxt::new();
    let bound = BoundTy {
        var: BoundVar(0),
        kind: BoundTyKind::Anon,
    };
    let a = interner.mk_ty(Ty::Bound(DebruijnIndex::INNERMOST, bound));
    let b = interner.mk_ty(Ty::Bound(DebruijnIndex::INNERMOST, bound));
    assert!(infcx.eq(&interner, a, b).is_ok());
}

#[test]
fn eq_bound_var_different_rejects() {
    let interner = Interner::new();
    let mut infcx = InferCtxt::new();
    let a = interner.mk_ty(Ty::Bound(
        DebruijnIndex::INNERMOST,
        BoundTy {
            var: BoundVar(0),
            kind: BoundTyKind::Anon,
        },
    ));
    let b = interner.mk_ty(Ty::Bound(
        DebruijnIndex::INNERMOST,
        BoundTy {
            var: BoundVar(1),
            kind: BoundTyKind::Anon,
        },
    ));
    assert!(infcx.eq(&interner, a, b).is_err());
}

#[test]
fn eq_generic_arg_kind_mismatch() {
    let interner = Interner::new();
    let mut infcx = InferCtxt::new();
    let t_i32 = interner.mk_ty(Ty::Int(IntTy::I32));
    let c = interner.mk_const_from_parts(Const::Value(ConstValue::Int(42)), t_i32);
    let args_a = interner.mk_generic_args(&[GenericArg::Type(t_i32)]);
    let args_b = interner.mk_generic_args(&[GenericArg::Const(c)]);
    match infcx.eq(
        &interner,
        interner.mk_ty(Ty::Tuple(args_a)),
        interner.mk_ty(Ty::Tuple(args_b)),
    ) {
        Err(TypeError::GenericArgKindMismatch { index }) => assert_eq!(index, 0),
        other => panic!("expected GenericArgKindMismatch, got {:?}", other),
    }
}

#[test]
fn eq_projection_trait_ref_mismatch() {
    let interner = Interner::new();
    let mut infcx = InferCtxt::new();
    let t_i32 = interner.mk_ty(Ty::Int(IntTy::I32));
    let trait_a = TraitRef {
        def_id: DefId::new(1),
        args: interner.mk_generic_args(&[GenericArg::Type(t_i32)]),
    };
    let trait_b = TraitRef {
        def_id: DefId::new(2),
        args: interner.mk_generic_args(&[GenericArg::Type(t_i32)]),
    };
    let proj_a = interner.mk_ty(Ty::Projection(ProjectionTy {
        trait_ref: trait_a,
        item_def_id: DefId::new(10),
    }));
    let proj_b = interner.mk_ty(Ty::Projection(ProjectionTy {
        trait_ref: trait_b,
        item_def_id: DefId::new(10),
    }));
    match infcx.eq(&interner, proj_a, proj_b) {
        Err(TypeError::TraitRefMismatch { expected, found }) => {
            assert_eq!(expected.def_id, DefId::new(1));
            assert_eq!(found.def_id, DefId::new(2));
        }
        other => panic!("expected TraitRefMismatch, got {:?}", other),
    }
}
