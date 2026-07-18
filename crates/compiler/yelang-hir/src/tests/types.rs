//! Exhaustive tests for AST type -> HIR type lowering.

use crate::hir::core::ItemKind;
use crate::hir::ty::{Ty, UtilityKind};
use crate::lowering::lower_crate;
use crate::res::Res;
use crate::tests::common::{parse_program, stub_resolved, stub_resolved_with_array};

fn get_fn_sig(crate_hir: &crate::Crate) -> &crate::hir::core::FnSig {
    let item = crate_hir
        .items
        .values()
        .find_map(|opt| {
            let item = opt.as_ref()?;
            matches!(item.kind, ItemKind::Fn { .. }).then_some(item)
        })
        .expect("expected a function item");
    let ItemKind::Fn { sig, .. } = &item.kind else {
        unreachable!()
    };
    sig
}

// ---------------------------------------------------------------------------
// Named / path types
// ---------------------------------------------------------------------------

#[test]
fn lower_named_type() {
    let src = "fn foo(x: i32) {}";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let sig = get_fn_sig(&crate_hir);
    let ty = crate_hir.ty(sig.inputs[0]).unwrap();
    assert!(matches!(ty, Ty::Path { .. }));
}

#[test]
fn lower_generic_type() {
    let src = "fn foo<T>(x: T) {}";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let sig = get_fn_sig(&crate_hir);
    let ty = crate_hir.ty(sig.inputs[0]).unwrap();
    assert!(matches!(ty, Ty::Path { .. }));
}

// ---------------------------------------------------------------------------
// Tuple
// ---------------------------------------------------------------------------

#[test]
fn lower_tuple_type() {
    let src = "fn foo(x: (i32, bool)) {}";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let sig = get_fn_sig(&crate_hir);
    let ty = crate_hir.ty(sig.inputs[0]).unwrap();
    match ty {
        Ty::Tuple { tys } => assert_eq!(tys.len(), 2),
        other => panic!("expected tuple type, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Array / Slice
// ---------------------------------------------------------------------------

#[test]
fn lower_array_type() {
    let src = "fn foo(x: [i32; 5]) {}";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let sig = get_fn_sig(&crate_hir);
    let ty = crate_hir.ty(sig.inputs[0]).unwrap();
    assert!(matches!(ty, Ty::Array { .. }));
}

#[test]
fn lower_dynamic_array_type() {
    // `[T]` is the surface syntax for the prelude `Array<T>` type.
    let src = "fn foo(x: [i32]) {}";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved_with_array(), &interner);

    let sig = get_fn_sig(&crate_hir);
    let ty = crate_hir.ty(sig.inputs[0]).unwrap();
    assert!(
        matches!(ty, Ty::Path { res, args } if matches!(res, Res::Def { .. }) && args.len() == 1),
        "expected [i32] to lower to Array<i32>, got {ty:?}"
    );
}

// ---------------------------------------------------------------------------
// Reference
// ---------------------------------------------------------------------------

#[test]
fn lower_ref_type() {
    let src = "fn foo(x: &i32) {}";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let sig = get_fn_sig(&crate_hir);
    let ty = crate_hir.ty(sig.inputs[0]).unwrap();
    match ty {
        Ty::Ref { mutability, .. } => {
            assert!(matches!(mutability, yelang_ast::Mutability::Immutable));
        }
        other => panic!("expected ref type, got {:?}", other),
    }
}

#[test]
fn lower_mut_ref_type() {
    let src = "fn foo(x: &mut i32) {}";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let sig = get_fn_sig(&crate_hir);
    let ty = crate_hir.ty(sig.inputs[0]).unwrap();
    match ty {
        Ty::Ref { mutability, .. } => {
            assert!(matches!(mutability, yelang_ast::Mutability::Mutable));
        }
        other => panic!("expected mut ref type, got {:?}", other),
    }
}

#[test]
fn lower_const_raw_ptr_type() {
    let src = "fn foo(x: *const i32) {}";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let sig = get_fn_sig(&crate_hir);
    let ty = crate_hir.ty(sig.inputs[0]).unwrap();
    match ty {
        Ty::RawPtr { mutability, .. } => {
            assert!(matches!(mutability, yelang_ast::Mutability::Immutable));
        }
        other => panic!("expected raw pointer type, got {:?}", other),
    }
}

#[test]
fn lower_mut_raw_ptr_type() {
    let src = "fn foo(x: *mut i32) {}";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let sig = get_fn_sig(&crate_hir);
    let ty = crate_hir.ty(sig.inputs[0]).unwrap();
    match ty {
        Ty::RawPtr { mutability, .. } => {
            assert!(matches!(mutability, yelang_ast::Mutability::Mutable));
        }
        other => panic!("expected raw pointer type, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Function pointer
// ---------------------------------------------------------------------------

#[test]
fn lower_fn_ptr_type() {
    let src = "fn foo(x: fn(i32) -> bool) {}";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let sig = get_fn_sig(&crate_hir);
    let ty = crate_hir.ty(sig.inputs[0]).unwrap();
    assert!(matches!(ty, Ty::FnPtr { .. }));
}

// ---------------------------------------------------------------------------
// HRTB / ForAll
// ---------------------------------------------------------------------------

#[test]
fn lower_forall_type() {
    let src = "fn foo(x: for<T> fn(T) -> T) {}";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let sig = get_fn_sig(&crate_hir);
    let ty = crate_hir.ty(sig.inputs[0]).unwrap();
    match ty {
        Ty::ForAll { params, ty } => {
            assert_eq!(params.len(), 1);
            let inner = crate_hir.ty(*ty).unwrap();
            assert!(matches!(inner, Ty::FnPtr { .. }));
        }
        other => panic!("expected forall type, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Literal type
// ---------------------------------------------------------------------------

#[test]
fn lower_literal_type() {
    let src = r#"fn foo(x: "hello") {}"#;
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let sig = get_fn_sig(&crate_hir);
    let ty = crate_hir.ty(sig.inputs[0]).unwrap();
    match ty {
        Ty::TypeLit { variants } => {
            assert_eq!(variants.len(), 1);
        }
        other => panic!("expected literal type, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Structural (anonymous struct)
// ---------------------------------------------------------------------------

#[test]
fn lower_structural_type() {
    let src = "fn foo(x: { a: i32, b: bool }) {}";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let sig = get_fn_sig(&crate_hir);
    let ty = crate_hir.ty(sig.inputs[0]).unwrap();
    match ty {
        Ty::AnonStruct { fields } => {
            assert_eq!(fields.len(), 2);
        }
        other => panic!("expected anon struct type, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Union
// ---------------------------------------------------------------------------

#[test]
fn lower_union_type() {
    let src = "fn foo(x: i32 | string | bool) {}";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let sig = get_fn_sig(&crate_hir);
    let ty = crate_hir.ty(sig.inputs[0]).unwrap();
    match ty {
        Ty::Union { tys } => {
            assert_eq!(tys.len(), 3);
        }
        other => panic!("expected union type, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Utility types
// ---------------------------------------------------------------------------

#[test]
fn lower_return_type_utility() {
    let src = "fn foo(x: ReturnType<typeof bar>) {}";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let sig = get_fn_sig(&crate_hir);
    let ty = crate_hir.ty(sig.inputs[0]).unwrap();
    match ty {
        Ty::Utility { kind, args } => {
            assert_eq!(*kind, UtilityKind::ReturnType);
            assert_eq!(args.len(), 1);
        }
        other => panic!("expected utility type, got {:?}", other),
    }
}

#[test]
fn lower_parameters_utility() {
    let src = "fn foo(x: Parameters<typeof bar>) {}";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let sig = get_fn_sig(&crate_hir);
    let ty = crate_hir.ty(sig.inputs[0]).unwrap();
    match ty {
        Ty::Utility { kind, args } => {
            assert_eq!(*kind, UtilityKind::Params);
            assert_eq!(args.len(), 1);
        }
        other => panic!("expected utility type, got {:?}", other),
    }
}

#[test]
fn lower_pick_utility() {
    let src = r#"fn foo(x: Pick<{ a: i32, b: string }, "a">) {}"#;
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let sig = get_fn_sig(&crate_hir);
    let ty = crate_hir.ty(sig.inputs[0]).unwrap();
    match ty {
        Ty::Utility { kind, args } => {
            assert_eq!(*kind, UtilityKind::Pick);
            assert_eq!(args.len(), 2);
        }
        other => panic!("expected utility type, got {:?}", other),
    }
}

#[test]
fn lower_omit_utility() {
    let src = r#"fn foo(x: Omit<{ a: i32, b: string }, "a">) {}"#;
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let sig = get_fn_sig(&crate_hir);
    let ty = crate_hir.ty(sig.inputs[0]).unwrap();
    match ty {
        Ty::Utility { kind, args } => {
            assert_eq!(*kind, UtilityKind::Omit);
            assert_eq!(args.len(), 2);
        }
        other => panic!("expected utility type, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// impl Trait / dyn Trait
// ---------------------------------------------------------------------------

#[test]
fn lower_impl_trait_type() {
    let src = "fn foo() -> impl Clone { 42 }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let sig = get_fn_sig(&crate_hir);
    let ty = crate_hir.ty(sig.output).unwrap();
    assert!(matches!(ty, Ty::ImplTrait { .. }));
}

#[test]
fn lower_dyn_trait_type() {
    let src = "fn foo(x: dyn Clone) {}";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let sig = get_fn_sig(&crate_hir);
    let ty = crate_hir.ty(sig.inputs[0]).unwrap();
    assert!(matches!(ty, Ty::DynTrait { .. }));
}

// ---------------------------------------------------------------------------
// Infer / Never
// ---------------------------------------------------------------------------

#[test]
fn lower_infer_type() {
    let src = "fn foo(x: _) {}";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let sig = get_fn_sig(&crate_hir);
    let ty = crate_hir.ty(sig.inputs[0]).unwrap();
    assert!(matches!(ty, Ty::Infer));
}

#[test]
fn lower_never_type() {
    let src = "fn foo() -> ! { panic!() }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let sig = get_fn_sig(&crate_hir);
    let ty = crate_hir.ty(sig.output).unwrap();
    assert!(matches!(ty, Ty::Never));
}

// ---------------------------------------------------------------------------
// Generics in items with complex types
// ---------------------------------------------------------------------------

#[test]
fn lower_fn_with_complex_generic_bounds() {
    let src = "fn process<T, U>(x: T, y: U) -> (T, U) where T: Clone, U: Display {}";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let item = crate_hir
        .items
        .values()
        .find_map(|opt| opt.as_ref())
        .unwrap();
    let ItemKind::Fn { generics, .. } = &item.kind else {
        panic!("expected fn")
    };
    assert_eq!(generics.params.len(), 2);
    assert!(generics.where_clause.is_some());
    let wc = generics.where_clause.as_ref().unwrap();
    assert_eq!(wc.predicates.len(), 2);
}

#[test]
fn lower_hrtb_where_predicate() {
    let src = "fn foo<T>() where for<U> T: Into<U> {}";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let item = crate_hir
        .items
        .values()
        .find_map(|opt| opt.as_ref())
        .unwrap();
    let ItemKind::Fn { generics, .. } = &item.kind else {
        panic!("expected fn")
    };
    let wc = generics
        .where_clause
        .as_ref()
        .expect("expected where clause");
    let pred = wc.predicates.first().expect("expected predicate");
    match pred {
        crate::hir::core::WherePredicate::TraitBound { ty, .. } => {
            let ty_node = crate_hir.ty(*ty).unwrap();
            assert!(matches!(ty_node, Ty::ForAll { .. }));
        }
        other => panic!("expected trait bound, got {:?}", other),
    }
}
