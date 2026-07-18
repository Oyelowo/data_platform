use yelang_arena::DefId;
use yelang_ty::canonical::{CanonicalVarValue, Certainty};
use yelang_ty::interner::Interner;
use yelang_ty::predicate::{ParamEnv, Predicate, TraitPredicate, TraitRef};
use yelang_ty::ty::{ImplPolarity, ParamTy, Ty, TyId};

use crate::eval_ctxt::EvalCtxt;
use crate::goal::Goal;
use crate::solver_ctx::BuiltinTraitKind;

use super::support::TestCtxt;

fn empty_env(_interner: &Interner) -> ParamEnv {
    ParamEnv {
        caller_bounds: yelang_ty::list::List::empty(),
    }
}

fn param_ty(interner: &Interner, index: u32) -> TyId {
    interner.mk_ty(Ty::Param(ParamTy {
        index,
        name: yelang_interner::Symbol::from(index),
    }))
}

fn trait_ref_with_self(interner: &Interner, trait_def_id: DefId, self_ty: TyId) -> TraitRef {
    TraitRef {
        def_id: trait_def_id,
        args: interner.mk_generic_args(&[yelang_ty::generic::GenericArg::Type(self_ty)]),
    }
}

fn add_simple_impl(cx: &mut TestCtxt<'_>, impl_def_id: DefId, trait_def_id: DefId, self_ty: TyId) {
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
fn response_var_values_resolved() {
    let interner = Interner::new();
    let mut cx = TestCtxt::new(&interner);
    let foo = DefId::new(1);
    cx.add_trait(foo, false);
    let i32_ty = cx.mk_i32();
    add_simple_impl(&mut cx, DefId::new(2), foo, i32_ty);

    let mut ecx = EvalCtxt::new(&interner, &cx);
    let unknown = ecx.infcx_mut().new_ty_var(&interner);
    let goal = cx.trait_goal(foo, unknown, empty_env(&interner));

    let response = ecx.evaluate_root_goal(goal).unwrap();
    assert_eq!(response.value.certainty, Certainty::Yes);
    assert_eq!(response.value.var_values.len(), 1);
    assert_eq!(
        response.value.var_values.as_slice()[0],
        CanonicalVarValue::Ty(i32_ty)
    );
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
        interner.mk_ty(Ty::Slice(cx.mk_i32())),
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
    let adt = interner.mk_ty(Ty::Adt(
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

    // struct Recursive { field: Recursive }
    // Goal: Recursive: Send
    // Structural derivation requires Recursive: Send, which is a coinductive
    // cycle and should succeed.
    let recursive_def = DefId::new(200);
    let recursive_ty = interner.mk_ty(Ty::Adt(
        yelang_ty::ty::AdtDef {
            def_id: recursive_def,
        },
        yelang_ty::list::List::empty(),
    ));
    cx.set_adt_fields(recursive_def, vec![recursive_ty]);

    let goal = cx.trait_goal(send, recursive_ty, empty_env(&interner));

    let mut ecx = EvalCtxt::new(&interner, &cx);
    let response = ecx.evaluate_root_goal(goal).unwrap();
    assert_eq!(response.value.certainty, Certainty::Yes);
}

#[test]
fn user_impl_preferred_over_auto_trait() {
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

    // Goal: Wrapper<i32>: Send
    // Both the user impl and structural auto-trait derivation apply; the solver
    // should prefer the user impl and avoid reporting ambiguity.
    let i32_ty = cx.mk_i32();
    let goal = cx.trait_goal(send, cx.mk_wrapper(i32_ty), empty_env(&interner));

    let mut ecx = EvalCtxt::new(&interner, &cx);
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

// -----------------------------------------------------------------------------
// Phase 5 — Projection normalization
// -----------------------------------------------------------------------------

#[test]
fn normalizes_to_simple_impl() {
    let interner = Interner::new();
    let mut cx = TestCtxt::new(&interner);

    let foo = DefId::new(300);
    let foo_item = DefId::new(301);
    cx.add_trait(foo, false);
    cx.add_trait_assoc_type(foo, foo_item, yelang_interner::Symbol::from(1u32));

    let i32_ty = cx.mk_i32();
    let bool_ty = interner.mk_ty(Ty::Bool);
    let impl_foo_i32 = DefId::new(302);
    cx.add_impl(impl_foo_i32, foo, i32_ty, 0, Vec::new());
    cx.add_impl_assoc_type(
        impl_foo_i32,
        DefId::new(3021),
        foo_item,
        yelang_interner::Symbol::from(1u32),
        bool_ty,
    );

    let goal = cx.normalizes_to_goal(foo, foo_item, i32_ty, bool_ty, empty_env(&interner));

    let mut ecx = EvalCtxt::new(&interner, &cx);
    let response = ecx.evaluate_root_goal(goal).unwrap();
    assert_eq!(response.value.certainty, Certainty::Yes);
}

#[test]
fn normalizes_to_generic_impl() {
    let interner = Interner::new();
    let mut cx = TestCtxt::new(&interner);

    let foo = DefId::new(300);
    let foo_item = DefId::new(301);
    let bar = DefId::new(304);
    cx.add_trait(foo, false);
    cx.add_trait(bar, false);
    cx.add_trait_assoc_type(foo, foo_item, yelang_interner::Symbol::from(1u32));

    // impl<T: Bar> Foo for Vec<T> { type Item = T; }
    let t = param_ty(&interner, 0);
    let vec_t = cx.mk_vec(t);
    let bar_t = Predicate::Trait(TraitPredicate {
        trait_ref: trait_ref_with_self(&interner, bar, t),
        polarity: ImplPolarity::Positive,
    });
    let impl_foo_vec = DefId::new(303);
    cx.add_impl(impl_foo_vec, foo, vec_t, 1, vec![bar_t]);
    cx.add_impl_assoc_type(
        impl_foo_vec,
        DefId::new(3031),
        foo_item,
        yelang_interner::Symbol::from(1u32),
        t,
    );

    // impl Bar for i32
    let i32_ty = cx.mk_i32();
    add_simple_impl(&mut cx, DefId::new(305), bar, i32_ty);

    // Goal: <Vec<i32> as Foo>::Item normalizes-to i32
    let goal = cx.normalizes_to_goal(
        foo,
        foo_item,
        cx.mk_vec(i32_ty),
        i32_ty,
        empty_env(&interner),
    );

    let mut ecx = EvalCtxt::new(&interner, &cx);
    let response = ecx.evaluate_root_goal(goal).unwrap();
    assert_eq!(response.value.certainty, Certainty::Yes);
}

#[test]
fn normalizes_to_no_impl_fails() {
    let interner = Interner::new();
    let mut cx = TestCtxt::new(&interner);

    let foo = DefId::new(300);
    let foo_item = DefId::new(301);
    cx.add_trait(foo, false);
    cx.add_trait_assoc_type(foo, foo_item, yelang_interner::Symbol::from(1u32));

    let goal = cx.normalizes_to_goal(
        foo,
        foo_item,
        cx.mk_i32(),
        interner.mk_ty(Ty::Bool),
        empty_env(&interner),
    );

    let mut ecx = EvalCtxt::new(&interner, &cx);
    assert!(ecx.evaluate_root_goal(goal).is_err());
}

#[test]
fn projection_equality_via_normalization() {
    let interner = Interner::new();
    let mut cx = TestCtxt::new(&interner);

    let foo = DefId::new(300);
    let foo_item = DefId::new(301);
    cx.add_trait(foo, false);
    cx.add_trait_assoc_type(foo, foo_item, yelang_interner::Symbol::from(1u32));

    let i32_ty = cx.mk_i32();
    let bool_ty = interner.mk_ty(Ty::Bool);
    let impl_foo_i32 = DefId::new(302);
    cx.add_impl(impl_foo_i32, foo, i32_ty, 0, Vec::new());
    cx.add_impl_assoc_type(
        impl_foo_i32,
        DefId::new(3021),
        foo_item,
        yelang_interner::Symbol::from(1u32),
        bool_ty,
    );

    let goal = cx.projection_goal(foo, foo_item, i32_ty, bool_ty, empty_env(&interner));

    let mut ecx = EvalCtxt::new(&interner, &cx);
    let response = ecx.evaluate_root_goal(goal).unwrap();
    assert_eq!(response.value.certainty, Certainty::Yes);
}

// -----------------------------------------------------------------------------
// Phase 5 — Auto-trait derivation
// -----------------------------------------------------------------------------

#[test]
fn auto_trait_adt() {
    let interner = Interner::new();
    let mut cx = TestCtxt::new(&interner);
    let send = DefId::new(400);
    cx.add_trait(send, true);

    // struct Point { x: i32, y: i32 }
    let point_def = DefId::new(401);
    let point_ty = cx.mk_adt(point_def, &[]);
    cx.set_adt_fields(point_def, vec![cx.mk_i32(), cx.mk_i32()]);

    let goal = cx.trait_goal(send, point_ty, empty_env(&interner));

    let mut ecx = EvalCtxt::new(&interner, &cx);
    let response = ecx.evaluate_root_goal(goal).unwrap();
    assert_eq!(response.value.certainty, Certainty::Yes);
}

#[test]
fn auto_trait_tuple() {
    let interner = Interner::new();
    let mut cx = TestCtxt::new(&interner);
    let send = DefId::new(400);
    cx.add_trait(send, true);

    let pair = interner.mk_ty(Ty::Tuple(interner.mk_generic_args(&[
        yelang_ty::generic::GenericArg::Type(cx.mk_i32()),
        yelang_ty::generic::GenericArg::Type(interner.mk_ty(Ty::Bool)),
    ])));

    let goal = cx.trait_goal(send, pair, empty_env(&interner));

    let mut ecx = EvalCtxt::new(&interner, &cx);
    let response = ecx.evaluate_root_goal(goal).unwrap();
    assert_eq!(response.value.certainty, Certainty::Yes);
}

#[test]
fn auto_trait_reference() {
    let interner = Interner::new();
    let mut cx = TestCtxt::new(&interner);
    let send = DefId::new(400);
    cx.add_trait(send, true);

    let ref_i32 = interner.mk_ty(Ty::Ref(cx.mk_i32(), yelang_ty::ty::Mutability::Not));

    let goal = cx.trait_goal(send, ref_i32, empty_env(&interner));

    let mut ecx = EvalCtxt::new(&interner, &cx);
    let response = ecx.evaluate_root_goal(goal).unwrap();
    assert_eq!(response.value.certainty, Certainty::Yes);
}

#[test]
fn auto_trait_nested_generic_adt() {
    let interner = Interner::new();
    let mut cx = TestCtxt::new(&interner);
    let send = DefId::new(400);
    cx.add_trait(send, true);

    // mk_pair builds Pair<Single<i32>> where:
    //   Single<T>  = Adt(102, [T]) with field T
    //   Pair<U>    = Adt(103, [U]) with field U
    let i32_ty = cx.mk_i32();
    let pair_ty = cx.mk_pair(i32_ty, i32_ty);

    let t = param_ty(&interner, 0);
    cx.set_adt_fields(DefId::new(102), vec![t]);
    cx.set_adt_fields(DefId::new(103), vec![cx.mk_adt(DefId::new(102), &[i32_ty])]);

    let goal = cx.trait_goal(send, pair_ty, empty_env(&interner));

    let mut ecx = EvalCtxt::new(&interner, &cx);
    let response = ecx.evaluate_root_goal(goal).unwrap();
    assert_eq!(response.value.certainty, Certainty::Yes);
}

// -----------------------------------------------------------------------------
// Phase 5 — Negative polarity
// -----------------------------------------------------------------------------

#[test]
fn negative_impl_proves_negative_goal() {
    let interner = Interner::new();
    let mut cx = TestCtxt::new(&interner);
    let foo = DefId::new(500);
    cx.add_trait(foo, false);

    let i32_ty = cx.mk_i32();
    cx.add_negative_impl(DefId::new(501), foo, i32_ty, 0, Vec::new());

    let goal = Goal::new(
        empty_env(&interner),
        Predicate::Trait(TraitPredicate {
            trait_ref: trait_ref_with_self(&interner, foo, i32_ty),
            polarity: ImplPolarity::Negative,
        }),
    );

    let mut ecx = EvalCtxt::new(&interner, &cx);
    let response = ecx.evaluate_root_goal(goal).unwrap();
    assert_eq!(response.value.certainty, Certainty::Yes);
}

#[test]
fn negative_goal_fails_without_negative_impl() {
    let interner = Interner::new();
    let mut cx = TestCtxt::new(&interner);
    let foo = DefId::new(500);
    cx.add_trait(foo, false);

    let i32_ty = cx.mk_i32();
    add_simple_impl(&mut cx, DefId::new(502), foo, i32_ty);

    let goal = Goal::new(
        empty_env(&interner),
        Predicate::Trait(TraitPredicate {
            trait_ref: trait_ref_with_self(&interner, foo, i32_ty),
            polarity: ImplPolarity::Negative,
        }),
    );

    let mut ecx = EvalCtxt::new(&interner, &cx);
    assert!(ecx.evaluate_root_goal(goal).is_err());
}

// -----------------------------------------------------------------------------
// Phase 5 — Supertrait elaboration
// -----------------------------------------------------------------------------

#[test]
fn supertrait_elaboration() {
    let interner = Interner::new();
    let mut cx = TestCtxt::new(&interner);

    let bar = DefId::new(600);
    let foo = DefId::new(601);
    cx.add_trait(bar, false);
    cx.add_trait(foo, false);

    // trait Foo: Bar
    let self_param = param_ty(&interner, 0);
    cx.add_trait_supertraits(foo, vec![trait_ref_with_self(&interner, bar, self_param)]);

    let i32_ty = cx.mk_i32();
    add_simple_impl(&mut cx, DefId::new(602), bar, i32_ty);
    add_simple_impl(&mut cx, DefId::new(603), foo, i32_ty);

    let goal = cx.trait_goal(foo, i32_ty, empty_env(&interner));

    let mut ecx = EvalCtxt::new(&interner, &cx);
    let response = ecx.evaluate_root_goal(goal).unwrap();
    assert_eq!(response.value.certainty, Certainty::Yes);
}

// -----------------------------------------------------------------------------
// Phase 5 — Blanket impls
// -----------------------------------------------------------------------------

#[test]
fn blanket_impl() {
    let interner = Interner::new();
    let mut cx = TestCtxt::new(&interner);

    let bar = DefId::new(700);
    let foo = DefId::new(701);
    cx.add_trait(bar, false);
    cx.add_trait(foo, false);

    // impl<T: Bar> Foo for T
    let t = param_ty(&interner, 0);
    let bar_t = Predicate::Trait(TraitPredicate {
        trait_ref: trait_ref_with_self(&interner, bar, t),
        polarity: ImplPolarity::Positive,
    });
    cx.add_impl(DefId::new(703), foo, t, 1, vec![bar_t]);

    let i32_ty = cx.mk_i32();
    add_simple_impl(&mut cx, DefId::new(702), bar, i32_ty);

    let goal = cx.trait_goal(foo, i32_ty, empty_env(&interner));

    let mut ecx = EvalCtxt::new(&interner, &cx);
    let response = ecx.evaluate_root_goal(goal).unwrap();
    assert_eq!(response.value.certainty, Certainty::Yes);
}

// -----------------------------------------------------------------------------
// Phase 5 — Ambiguity stalling
// -----------------------------------------------------------------------------

#[test]
fn ambiguous_nested_goal_stalls_to_maybe() {
    let interner = Interner::new();
    let mut cx = TestCtxt::new(&interner);

    let foo = DefId::new(800);
    let bar = DefId::new(801);
    cx.add_trait(foo, false);
    cx.add_trait(bar, false);

    // impl<T: Bar> Foo for Vec<T>
    let t = param_ty(&interner, 0);
    let vec_t = cx.mk_vec(t);
    let bar_t = Predicate::Trait(TraitPredicate {
        trait_ref: trait_ref_with_self(&interner, bar, t),
        polarity: ImplPolarity::Positive,
    });
    cx.add_impl(DefId::new(802), foo, vec_t, 1, vec![bar_t]);

    // Goal: Vec<?T>: Foo where ?T is unknown => ambiguous because ?T: Bar is ambiguous.
    let mut ecx = EvalCtxt::new(&interner, &cx);
    let unknown = ecx.infcx_mut().new_ty_var(&interner);
    let goal = cx.trait_goal(foo, cx.mk_vec(unknown), empty_env(&interner));

    let response = ecx.evaluate_root_goal(goal).unwrap();
    assert_eq!(response.value.certainty, Certainty::Maybe);
}
