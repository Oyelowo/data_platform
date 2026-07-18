use yelang_arena::DefId;
use yelang_ty::canonical::Certainty;
use yelang_ty::interner::Interner;
use yelang_ty::predicate::{ParamEnv, Predicate, TraitPredicate, TraitRef};
use yelang_ty::ty::{ImplPolarity, ParamTy, Ty, TyKind};

use crate::eval_ctxt::EvalCtxt;
use crate::goal::Goal;
use crate::solver_ctx::BuiltinTraitKind;

use super::support::TestCtxt;

fn empty_env<'tcx>(_interner: &'tcx Interner<'tcx>) -> ParamEnv<'tcx> {
    ParamEnv {
        caller_bounds: yelang_ty::list::List::empty(),
    }
}

fn param_ty<'tcx>(interner: &'tcx Interner<'tcx>, index: u32) -> Ty<'tcx> {
    interner.mk_ty(TyKind::Param(ParamTy {
        index,
        name: yelang_interner::Symbol::from(index),
    }))
}

fn trait_ref_with_self<'tcx>(
    interner: &'tcx Interner<'tcx>,
    trait_def_id: DefId,
    self_ty: Ty<'tcx>,
) -> TraitRef<'tcx> {
    TraitRef {
        def_id: trait_def_id,
        args: interner.mk_generic_args(&[yelang_ty::generic::GenericArg::Type(self_ty)]),
    }
}

fn add_simple_impl<'tcx>(
    cx: &mut TestCtxt<'tcx>,
    impl_def_id: DefId,
    trait_def_id: DefId,
    self_ty: Ty<'tcx>,
) {
    cx.add_impl(impl_def_id, trait_def_id, self_ty, 0, Vec::new());
}

#[test]
fn param_env_success() {
    let interner = Interner::new();
    let mut cx = TestCtxt::new(&interner);
    let foo = DefId::new(1);
    cx.add_trait(foo, false);

    let i32_ty = cx.mk_i32();
    let assumption = Predicate::Trait(TraitPredicate {
        trait_ref: trait_ref_with_self(&interner, foo, i32_ty),
        polarity: ImplPolarity::Positive,
    });
    let env = cx.param_env(&[assumption]);
    let goal = cx.trait_goal(foo, i32_ty, env);

    let mut ecx = EvalCtxt::new(&interner, &cx);
    let response = ecx.evaluate_root_goal(goal).unwrap();
    assert_eq!(response.value.certainty, Certainty::Yes);
}

#[test]
fn param_env_failure_no_assumption() {
    let interner = Interner::new();
    let mut cx = TestCtxt::new(&interner);
    let foo = DefId::new(1);
    cx.add_trait(foo, false);

    let goal = cx.trait_goal(foo, cx.mk_i32(), empty_env(&interner));

    let mut ecx = EvalCtxt::new(&interner, &cx);
    assert!(ecx.evaluate_root_goal(goal).is_err());
}

#[test]
fn simple_user_impl() {
    let interner = Interner::new();
    let mut cx = TestCtxt::new(&interner);
    let foo = DefId::new(1);
    cx.add_trait(foo, false);
    let i32_ty = cx.mk_i32();
    add_simple_impl(&mut cx, DefId::new(2), foo, i32_ty);

    let goal = cx.trait_goal(foo, i32_ty, empty_env(&interner));

    let mut ecx = EvalCtxt::new(&interner, &cx);
    let response = ecx.evaluate_root_goal(goal).unwrap();
    assert_eq!(response.value.certainty, Certainty::Yes);
}

#[test]
fn generic_impl_with_nested_obligation() {
    let interner = Interner::new();
    let mut cx = TestCtxt::new(&interner);
    let foo = DefId::new(1);
    let bar = DefId::new(2);
    cx.add_trait(foo, false);
    cx.add_trait(bar, false);

    // impl<T> Foo for Vec<T> where T: Bar
    let t = param_ty(&interner, 0);
    let vec_t = cx.mk_vec(t);
    let bar_t = Predicate::Trait(TraitPredicate {
        trait_ref: trait_ref_with_self(&interner, bar, t),
        polarity: ImplPolarity::Positive,
    });
    cx.add_impl(DefId::new(10), foo, vec_t, 1, vec![bar_t]);

    // impl Bar for i32
    let i32_ty = cx.mk_i32();
    add_simple_impl(&mut cx, DefId::new(11), bar, i32_ty);

    let goal = cx.trait_goal(foo, cx.mk_vec(i32_ty), empty_env(&interner));

    let mut ecx = EvalCtxt::new(&interner, &cx);
    let response = ecx.evaluate_root_goal(goal).unwrap();
    assert_eq!(response.value.certainty, Certainty::Yes);
}

#[test]
fn generic_impl_where_clause_fails() {
    let interner = Interner::new();
    let mut cx = TestCtxt::new(&interner);
    let foo = DefId::new(1);
    let bar = DefId::new(2);
    cx.add_trait(foo, false);
    cx.add_trait(bar, false);

    let t = param_ty(&interner, 0);
    let vec_t = cx.mk_vec(t);
    let bar_t = Predicate::Trait(TraitPredicate {
        trait_ref: trait_ref_with_self(&interner, bar, t),
        polarity: ImplPolarity::Positive,
    });
    cx.add_impl(DefId::new(10), foo, vec_t, 1, vec![bar_t]);

    // No impl for Bar.

    let goal = cx.trait_goal(foo, cx.mk_vec(cx.mk_i32()), empty_env(&interner));

    let mut ecx = EvalCtxt::new(&interner, &cx);
    assert!(ecx.evaluate_root_goal(goal).is_err());
}

#[test]
fn builtin_sized() {
    let interner = Interner::new();
    let mut cx = TestCtxt::new(&interner);
    let sized = DefId::new(3);
    cx.add_builtin(sized, BuiltinTraitKind::Sized);

    let goal = cx.trait_goal(sized, cx.mk_i32(), empty_env(&interner));
    let mut ecx = EvalCtxt::new(&interner, &cx);
    assert_eq!(
        ecx.evaluate_root_goal(goal).unwrap().value.certainty,
        Certainty::Yes
    );

    let slice_goal = cx.trait_goal(
        sized,
        interner.mk_ty(TyKind::Slice(cx.mk_i32())),
        empty_env(&interner),
    );
    assert!(ecx.evaluate_root_goal(slice_goal).is_err());
}

#[test]
fn builtin_copy_conservative() {
    let interner = Interner::new();
    let mut cx = TestCtxt::new(&interner);
    let copy = DefId::new(4);
    cx.add_builtin(copy, BuiltinTraitKind::Copy);

    let goal = cx.trait_goal(copy, cx.mk_i32(), empty_env(&interner));
    let mut ecx = EvalCtxt::new(&interner, &cx);
    assert_eq!(
        ecx.evaluate_root_goal(goal).unwrap().value.certainty,
        Certainty::Yes
    );

    // ADTs are not built-in Copy.
    let adt = interner.mk_ty(TyKind::Adt(
        yelang_ty::ty::AdtDef {
            def_id: DefId::new(99),
        },
        yelang_ty::list::List::empty(),
    ));
    let adt_goal = cx.trait_goal(copy, adt, empty_env(&interner));
    assert!(ecx.evaluate_root_goal(adt_goal).is_err());
}

#[test]
fn inductive_cycle_returns_overflow() {
    let interner = Interner::new();
    let mut cx = TestCtxt::new(&interner);
    let a = DefId::new(10);
    cx.add_trait(a, false);

    // impl<T: A> A for Wrapper<T>
    let t = param_ty(&interner, 0);
    let wrapper_t = cx.mk_wrapper(t);
    let nested = Predicate::Trait(TraitPredicate {
        trait_ref: trait_ref_with_self(&interner, a, t),
        polarity: ImplPolarity::Positive,
    });
    cx.add_impl(DefId::new(11), a, wrapper_t, 1, vec![nested]);

    // Goal: Wrapper<?T>: A
    let mut ecx = EvalCtxt::new(&interner, &cx);
    let unknown = ecx.infcx_mut().new_ty_var(&interner);
    let goal = cx.trait_goal(a, cx.mk_wrapper(unknown), empty_env(&interner));

    let response = ecx.evaluate_root_goal(goal).unwrap();
    assert_eq!(response.value.certainty, Certainty::Maybe);
}

#[test]
fn coinductive_auto_trait_cycle_succeeds() {
    let interner = Interner::new();
    let mut cx = TestCtxt::new(&interner);
    let send = DefId::new(20);
    cx.add_trait(send, true);

    // impl<T: Send> Send for Wrapper<T>
    let t = param_ty(&interner, 0);
    let wrapper_t = cx.mk_wrapper(t);
    let nested = Predicate::Trait(TraitPredicate {
        trait_ref: trait_ref_with_self(&interner, send, t),
        polarity: ImplPolarity::Positive,
    });
    cx.add_impl(DefId::new(21), send, wrapper_t, 1, vec![nested]);

    let mut ecx = EvalCtxt::new(&interner, &cx);
    let unknown = ecx.infcx_mut().new_ty_var(&interner);
    let goal = cx.trait_goal(send, cx.mk_wrapper(unknown), empty_env(&interner));

    let response = ecx.evaluate_root_goal(goal).unwrap();
    assert_eq!(response.value.certainty, Certainty::Yes);
}

#[test]
fn depth_limit_overflow() {
    let interner = Interner::new();
    let mut cx = TestCtxt::new(&interner);
    let a = DefId::new(30);
    let b = DefId::new(31);
    cx.add_trait(a, false);
    cx.add_trait(b, false);

    // impl<T> A for Vec<T> where Vec<T>: B
    let t = param_ty(&interner, 0);
    let vec_t = cx.mk_vec(t);
    let b_t = Predicate::Trait(TraitPredicate {
        trait_ref: trait_ref_with_self(&interner, b, vec_t),
        polarity: ImplPolarity::Positive,
    });
    cx.add_impl(DefId::new(32), a, vec_t, 1, vec![b_t]);

    // impl<T> B for Vec<T> where Vec<T>: A
    let b_nested = Predicate::Trait(TraitPredicate {
        trait_ref: trait_ref_with_self(&interner, a, vec_t),
        polarity: ImplPolarity::Positive,
    });
    cx.add_impl(DefId::new(33), b, vec_t, 1, vec![b_nested]);

    // Goal Vec<i32>: A requires B<i32> requires A<i32>, ad infinitum.
    let goal = cx.trait_goal(a, cx.mk_vec(cx.mk_i32()), empty_env(&interner));

    let mut ecx = EvalCtxt::with_max_depth(&interner, &cx, 4);
    let response = ecx.evaluate_root_goal(goal).unwrap();
    assert_eq!(response.value.certainty, Certainty::Maybe);
}

#[test]
fn cache_reuses_result() {
    let interner = Interner::new();
    let mut cx = TestCtxt::new(&interner);
    let foo = DefId::new(1);
    cx.add_trait(foo, false);
    let i32_ty = cx.mk_i32();
    add_simple_impl(&mut cx, DefId::new(2), foo, i32_ty);

    let goal = cx.trait_goal(foo, i32_ty, empty_env(&interner));

    let mut ecx = EvalCtxt::new(&interner, &cx);
    let r1 = ecx.evaluate_root_goal(goal).unwrap();
    let r2 = ecx.evaluate_root_goal(goal).unwrap();
    assert_eq!(r1.value.certainty, Certainty::Yes);
    assert_eq!(r2.value.certainty, Certainty::Yes);
}

#[test]
fn overlapping_impls_are_ambiguous() {
    let interner = Interner::new();
    let mut cx = TestCtxt::new(&interner);
    let foo = DefId::new(1);
    cx.add_trait(foo, false);
    let i32_ty = cx.mk_i32();
    add_simple_impl(&mut cx, DefId::new(2), foo, i32_ty);
    add_simple_impl(&mut cx, DefId::new(3), foo, i32_ty);

    let goal = cx.trait_goal(foo, i32_ty, empty_env(&interner));

    let mut ecx = EvalCtxt::new(&interner, &cx);
    let response = ecx.evaluate_root_goal(goal).unwrap();
    assert_eq!(response.value.certainty, Certainty::Maybe);
}

#[test]
fn no_impl_is_no_solution() {
    let interner = Interner::new();
    let mut cx = TestCtxt::new(&interner);
    let foo = DefId::new(1);
    cx.add_trait(foo, false);

    let goal = cx.trait_goal(foo, cx.mk_i32(), empty_env(&interner));

    let mut ecx = EvalCtxt::new(&interner, &cx);
    assert!(ecx.evaluate_root_goal(goal).is_err());
}
