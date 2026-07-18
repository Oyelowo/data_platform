use yelang_arena::DefId;
use yelang_infer::InferCtxt;
use yelang_interner::Symbol;
use yelang_ty::binder::{BoundTy, BoundTyKind, BoundVar, DebruijnIndex};
use yelang_ty::canonical::CanonicalVarKind;
use yelang_ty::interner::Interner;
use yelang_ty::list::List;
use yelang_ty::predicate::{ParamEnv, Predicate, TraitPredicate, TraitRef};
use yelang_ty::primitive::IntTy;
use yelang_ty::ty::{Const, InferTy, PlaceholderType, Ty, TyId, UniverseIndex};

use crate::canonicalize::canonicalize;
use crate::goal::Goal;
use crate::instantiate::instantiate;

fn trait_goal(interner: &Interner, ty: TyId) -> Goal {
    let args = interner.mk_generic_args(&[yelang_ty::generic::GenericArg::Type(ty)]);
    let trait_ref = TraitRef {
        def_id: DefId::new(1),
        args,
    };
    Goal {
        param_env: ParamEnv {
            caller_bounds: List::empty(),
        },
        predicate: Predicate::Trait(TraitPredicate {
            trait_ref,
            polarity: yelang_ty::ty::ImplPolarity::Positive,
        }),
    }
}

#[test]
fn canonicalize_ty_var_becomes_bound() {
    let interner = Interner::new();
    let mut infcx = InferCtxt::new();
    let ty_var = infcx.new_ty_var(&interner);

    let canonical = canonicalize(ty_var, &interner, &mut infcx, UniverseIndex(0));

    assert_eq!(canonical.variables.len(), 1);
    assert!(matches!(
        canonical.variables.as_slice()[0],
        CanonicalVarKind::Ty(_)
    ));
    assert!(matches!(
        interner.ty(canonical.value),
        Ty::Bound(
            DebruijnIndex(0),
            BoundTy {
                var: BoundVar(0),
                ..
            }
        )
    ));
}

#[test]
fn canonicalize_int_var_becomes_int_kind() {
    let interner = Interner::new();
    let mut infcx = InferCtxt::new();
    let int_var = infcx.new_int_var(&interner);

    let canonical = canonicalize(int_var, &interner, &mut infcx, UniverseIndex(0));

    assert_eq!(canonical.variables.len(), 1);
    assert_eq!(canonical.variables.as_slice()[0], CanonicalVarKind::Int);
}

#[test]
fn canonicalize_float_var_becomes_float_kind() {
    let interner = Interner::new();
    let mut infcx = InferCtxt::new();
    let float_var = infcx.new_float_var(&interner);

    let canonical = canonicalize(float_var, &interner, &mut infcx, UniverseIndex(0));

    assert_eq!(canonical.variables.len(), 1);
    assert_eq!(canonical.variables.as_slice()[0], CanonicalVarKind::Float);
}

#[test]
fn canonicalize_const_var_becomes_const_kind() {
    let interner = Interner::new();
    let mut infcx = InferCtxt::new();
    let ty = interner.mk_ty(Ty::Int(IntTy::I32));
    let const_var = infcx.new_const_var(&interner, ty);

    let canonical = canonicalize(const_var, &interner, &mut infcx, UniverseIndex(0));

    assert_eq!(canonical.variables.len(), 1);
    assert_eq!(canonical.variables.as_slice()[0], CanonicalVarKind::Const);
    assert!(matches!(
        interner.const_kind(canonical.value),
        Const::Bound(DebruijnIndex(0), BoundVar(0))
    ));
}

#[test]
fn canonicalize_placeholder_preserves_universe() {
    let interner = Interner::new();
    let mut infcx = InferCtxt::new();
    let placeholder = PlaceholderType {
        universe: UniverseIndex(2),
        name: Symbol::from(7),
    };
    let ty = interner.mk_ty(Ty::Placeholder(placeholder));

    let canonical = canonicalize(ty, &interner, &mut infcx, UniverseIndex(2));

    assert_eq!(canonical.variables.len(), 1);
    assert_eq!(
        canonical.variables.as_slice()[0],
        CanonicalVarKind::PlaceholderTy(placeholder)
    );
}

#[test]
fn canonicalize_shifts_existing_bound_vars() {
    let interner = Interner::new();
    let mut infcx = InferCtxt::new();
    let bound = interner.mk_ty(Ty::Bound(
        DebruijnIndex::INNERMOST,
        BoundTy {
            var: BoundVar(0),
            kind: BoundTyKind::Anon,
        },
    ));

    let canonical = canonicalize(bound, &interner, &mut infcx, UniverseIndex(0));

    assert!(matches!(
        interner.ty(canonical.value),
        Ty::Bound(
            DebruijnIndex(1),
            BoundTy {
                var: BoundVar(0),
                ..
            }
        )
    ));
}

#[test]
fn canonicalize_shared_var_uses_same_index() {
    let interner = Interner::new();
    let mut infcx = InferCtxt::new();
    let ty_var = infcx.new_ty_var(&interner);
    let pair = interner.mk_ty(Ty::Tuple(interner.mk_generic_args(&[
        yelang_ty::generic::GenericArg::Type(ty_var),
        yelang_ty::generic::GenericArg::Type(ty_var),
    ])));

    let canonical = canonicalize(pair, &interner, &mut infcx, UniverseIndex(0));

    assert_eq!(canonical.variables.len(), 1);
    if let Ty::Tuple(args) = interner.ty(canonical.value) {
        assert!(args.iter().all(|arg| matches!(
            interner.ty(arg.expect_type()),
            Ty::Bound(
                DebruijnIndex(0),
                BoundTy {
                    var: BoundVar(0),
                    ..
                }
            )
        )));
    } else {
        panic!("expected tuple");
    }
}

#[test]
fn canonicalize_distinct_vars_use_distinct_indices() {
    let interner = Interner::new();
    let mut infcx = InferCtxt::new();
    let a = infcx.new_ty_var(&interner);
    let b = infcx.new_ty_var(&interner);
    let pair = interner.mk_ty(Ty::Tuple(interner.mk_generic_args(&[
        yelang_ty::generic::GenericArg::Type(a),
        yelang_ty::generic::GenericArg::Type(b),
    ])));

    let canonical = canonicalize(pair, &interner, &mut infcx, UniverseIndex(0));

    assert_eq!(canonical.variables.len(), 2);
    if let Ty::Tuple(args) = interner.ty(canonical.value) {
        let first = match interner.ty(args[0].expect_type()) {
            Ty::Bound(_, BoundTy { var, .. }) => var.0,
            _ => panic!("expected bound"),
        };
        let second = match interner.ty(args[1].expect_type()) {
            Ty::Bound(_, BoundTy { var, .. }) => var.0,
            _ => panic!("expected bound"),
        };
        assert_ne!(first, second);
    } else {
        panic!("expected tuple");
    }
}

#[test]
fn canonicalize_resolved_ty_var() {
    let interner = Interner::new();
    let mut infcx = InferCtxt::new();
    let ty_var = infcx.new_ty_var(&interner);
    let i32_ty = interner.mk_ty(Ty::Int(IntTy::I32));
    infcx.eq(&interner, ty_var, i32_ty).unwrap();

    let canonical = canonicalize(ty_var, &interner, &mut infcx, UniverseIndex(0));

    assert_eq!(canonical.variables.len(), 0);
    assert_eq!(canonical.value, i32_ty);
}

#[test]
fn instantiate_creates_fresh_ty_var() {
    let interner = Interner::new();
    let mut infcx = InferCtxt::new();
    let ty_var = infcx.new_ty_var(&interner);
    let canonical = canonicalize(ty_var, &interner, &mut infcx, UniverseIndex(0));

    let mut fresh_infcx = InferCtxt::new();
    let instantiated = instantiate(canonical, &interner, &mut fresh_infcx);

    assert!(matches!(
        interner.ty(instantiated),
        Ty::Infer(InferTy::TyVar(_))
    ));
}

#[test]
fn instantiate_creates_fresh_placeholder() {
    let interner = Interner::new();
    let mut infcx = InferCtxt::new();
    let placeholder = PlaceholderType {
        universe: UniverseIndex(1),
        name: Symbol::from(42),
    };
    let ty = interner.mk_ty(Ty::Placeholder(placeholder));
    let canonical = canonicalize(ty, &interner, &mut infcx, UniverseIndex(1));

    let mut fresh_infcx = InferCtxt::new();
    let instantiated = instantiate(canonical, &interner, &mut fresh_infcx);

    if let Ty::Placeholder(p) = interner.ty(instantiated) {
        assert_eq!(p.universe, UniverseIndex(1));
        assert_ne!(p.name, placeholder.name);
    } else {
        panic!("expected placeholder");
    }
}

#[test]
fn instantiate_shifts_bound_vars_back_in() {
    let interner = Interner::new();
    let mut infcx = InferCtxt::new();
    let bound = interner.mk_ty(Ty::Bound(
        DebruijnIndex::INNERMOST,
        BoundTy {
            var: BoundVar(0),
            kind: BoundTyKind::Anon,
        },
    ));
    let canonical = canonicalize(bound, &interner, &mut infcx, UniverseIndex(0));

    let mut fresh_infcx = InferCtxt::new();
    let instantiated = instantiate(canonical, &interner, &mut fresh_infcx);

    assert!(matches!(
        interner.ty(instantiated),
        Ty::Bound(
            DebruijnIndex(0),
            BoundTy {
                var: BoundVar(0),
                ..
            }
        )
    ));
}

#[test]
fn canonicalize_goal_includes_param_env() {
    let interner = Interner::new();
    let mut infcx = InferCtxt::new();
    let ty_var = infcx.new_ty_var(&interner);
    let goal = trait_goal(&interner, ty_var);

    let canonical = canonicalize(goal, &interner, &mut infcx, UniverseIndex(0));

    assert_eq!(canonical.variables.len(), 1);
    assert!(matches!(
        canonical.value.predicate,
        Predicate::Trait(TraitPredicate { .. })
    ));
}
