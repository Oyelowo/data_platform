//! Integration tests for built-in derive expansion.

use crate::crate_hir::Crate;
use crate::hir::ItemKind;
use crate::hir_item::Item;
use crate::lowering::{LoweringContext, lower_crate};
use crate::lowering_err::LoweringError;
use crate::res::ResolvedCrate;
use yelang_interner::Interner;

fn lower_with_derives(src: &str) -> (Crate, Interner, ResolvedCrate, Vec<LoweringError>) {
    let interner = Interner::new();
    let mut stream = yelang_lexer::TokenKind::tokenize(src, &interner).expect("tokenize");
    let program = stream
        .parse::<yelang_ast::Program>()
        .expect("parse program");
    let resolved = yelang_resolve::resolve_crate(&program, &interner);
    let crate_hir = lower_crate(&program, &resolved, &interner);
    let errors = collect_lowering_errors(&program, &resolved, &interner);
    (crate_hir, interner, resolved, errors)
}

fn collect_lowering_errors(
    program: &yelang_ast::Program,
    resolved: &ResolvedCrate,
    interner: &Interner,
) -> Vec<LoweringError> {
    let mut ctx = LoweringContext::new(interner, resolved);
    for item in &program.items {
        let _ = crate::lowering_item::lower_item(&mut ctx, item);
    }
    ctx.errors
}

fn find_impls_for_type<'a>(
    crate_hir: &'a Crate,
    type_name: &str,
    interner: &Interner,
    resolved: &ResolvedCrate,
) -> Vec<&'a Item> {
    crate_hir
        .items
        .values()
        .filter_map(|opt| opt.as_ref())
        .filter(|item| {
            if let ItemKind::Impl {
                self_ty, of_trait, ..
            } = &item.kind
            {
                let name = match &self_ty.kind {
                    crate::hir_ty::TyKind::Path { res, .. } => match res {
                        crate::res::Res::Def { def_id } => resolved
                            .definitions
                            .get(*def_id)
                            .map(|d| interner.resolve(&d.name))
                            .unwrap_or(""),
                        _ => "",
                    },
                    _ => "",
                };
                name == type_name && of_trait.is_some()
            } else {
                false
            }
        })
        .collect()
}

#[test]
fn derive_copy_generates_empty_impl() {
    let src = r#"
        @derive(Copy)
        struct Point { x: i32, y: i32 }
    "#;
    let (crate_hir, interner, resolved, _errors) = lower_with_derives(src);
    let impls = find_impls_for_type(&crate_hir, "Point", &interner, &resolved);
    assert_eq!(impls.len(), 1, "expected one derived impl for Point");
    let ItemKind::Impl {
        items, of_trait, ..
    } = &impls[0].kind
    else {
        panic!("expected impl item");
    };
    assert!(items.is_empty(), "Copy impl should have no items");
    assert!(of_trait.is_some(), "Copy impl should implement a trait");
}

#[test]
fn derive_clone_generates_clone_method() {
    let src = r#"
        @derive(Clone)
        struct Point { x: i32, y: i32 }
    "#;
    let (crate_hir, interner, resolved, _errors) = lower_with_derives(src);
    let impls = find_impls_for_type(&crate_hir, "Point", &interner, &resolved);
    assert_eq!(impls.len(), 1);
    let ItemKind::Impl { items, .. } = &impls[0].kind else {
        panic!("expected impl item");
    };
    assert_eq!(items.len(), 1);
    assert_eq!(interner.resolve(&items[0].ident.symbol), "clone");
}

#[test]
fn derive_partial_eq_generates_eq_method() {
    let src = r#"
        @derive(PartialEq)
        struct Point { x: i32, y: i32 }
    "#;
    let (crate_hir, interner, resolved, _errors) = lower_with_derives(src);
    let impls = find_impls_for_type(&crate_hir, "Point", &interner, &resolved);
    assert_eq!(impls.len(), 1);
    let ItemKind::Impl { items, .. } = &impls[0].kind else {
        panic!("expected impl item");
    };
    assert_eq!(items.len(), 1);
    assert_eq!(interner.resolve(&items[0].ident.symbol), "eq");
}

#[test]
fn derive_eq_requires_partial_eq() {
    let src = r#"
        @derive(Eq)
        struct Point { x: i32, y: i32 }
    "#;
    let (crate_hir, _interner, _resolved, errors) = lower_with_derives(src);
    // Without PartialEq in the same attribute, Eq should emit an error and not generate an impl.
    let point_impls: Vec<_> = crate_hir
        .items
        .values()
        .filter_map(|opt| opt.as_ref())
        .filter(|item| matches!(item.kind, ItemKind::Impl { .. }))
        .collect();
    assert!(
        point_impls.is_empty(),
        "Eq derive without PartialEq should not generate impls"
    );
    assert!(
        !errors.is_empty(),
        "Eq derive without PartialEq should emit an error"
    );
}

#[test]
fn derive_copy_clone_partial_eq_eq_together() {
    let src = r#"
        @derive(Copy, Clone, PartialEq, Eq)
        struct Point { x: i32, y: i32 }
    "#;
    let (crate_hir, interner, resolved, _errors) = lower_with_derives(src);
    let impls = find_impls_for_type(&crate_hir, "Point", &interner, &resolved);
    assert_eq!(impls.len(), 4, "expected four derived impls");
}

#[test]
fn derive_clone_on_tuple_struct() {
    let src = r#"
        @derive(Clone)
        struct Pair(i32, i32);
    "#;
    let (crate_hir, interner, resolved, _errors) = lower_with_derives(src);
    let impls = find_impls_for_type(&crate_hir, "Pair", &interner, &resolved);
    assert_eq!(impls.len(), 1);
    let ItemKind::Impl { items, .. } = &impls[0].kind else {
        panic!("expected impl");
    };
    assert_eq!(items.len(), 1);
}

#[test]
fn derive_clone_on_unit_struct() {
    let src = r#"
        @derive(Clone)
        struct Unit;
    "#;
    let (crate_hir, interner, resolved, _errors) = lower_with_derives(src);
    let impls = find_impls_for_type(&crate_hir, "Unit", &interner, &resolved);
    assert_eq!(impls.len(), 1);
}

#[test]
fn derive_clone_on_enum() {
    let src = r#"
        @derive(Clone)
        enum E { A, B(i32), C { x: i32 } }
    "#;
    let (crate_hir, interner, resolved, _errors) = lower_with_derives(src);
    let impls = find_impls_for_type(&crate_hir, "E", &interner, &resolved);
    assert_eq!(impls.len(), 1);
    let ItemKind::Impl { items, .. } = &impls[0].kind else {
        panic!("expected impl");
    };
    assert_eq!(items.len(), 1);
}

#[test]
fn derive_partial_eq_on_enum() {
    let src = r#"
        @derive(PartialEq)
        enum E { A, B(i32), C { x: i32 } }
    "#;
    let (crate_hir, interner, resolved, _errors) = lower_with_derives(src);
    let impls = find_impls_for_type(&crate_hir, "E", &interner, &resolved);
    assert_eq!(impls.len(), 1);
    let ItemKind::Impl { items, .. } = &impls[0].kind else {
        panic!("expected impl");
    };
    assert_eq!(items.len(), 1);
}

#[test]
fn derive_debug_generates_fmt_method() {
    let src = r#"
        @derive(Debug)
        struct Point { x: i32, y: i32 }
    "#;
    let (crate_hir, interner, resolved, _errors) = lower_with_derives(src);
    let impls = find_impls_for_type(&crate_hir, "Point", &interner, &resolved);
    assert_eq!(impls.len(), 1);
    let ItemKind::Impl { items, .. } = &impls[0].kind else {
        panic!("expected impl");
    };
    assert_eq!(items.len(), 1);
    assert_eq!(interner.resolve(&items[0].ident.symbol), "fmt");
}

#[test]
fn derive_debug_on_enum() {
    let src = r#"
        @derive(Debug)
        enum E { A, B(i32), C { x: i32 } }
    "#;
    let (crate_hir, interner, resolved, _errors) = lower_with_derives(src);
    let impls = find_impls_for_type(&crate_hir, "E", &interner, &resolved);
    assert_eq!(impls.len(), 1);
}

#[test]
fn derive_all_mvp_together() {
    let src = r#"
        @derive(Copy, Clone, PartialEq, Eq, Debug)
        struct Point { x: i32, y: i32 }
    "#;
    let (crate_hir, interner, resolved, _errors) = lower_with_derives(src);
    let impls = find_impls_for_type(&crate_hir, "Point", &interner, &resolved);
    assert_eq!(impls.len(), 5);
}

#[test]
fn derive_copy_rejects_non_copy_field() {
    let src = r#"
        @derive(Copy)
        struct Bad { s: String }
    "#;
    let (crate_hir, _interner, _resolved, errors) = lower_with_derives(src);
    let impls: Vec<_> = crate_hir
        .items
        .values()
        .filter_map(|opt| opt.as_ref())
        .filter(|item| matches!(item.kind, ItemKind::Impl { .. }))
        .collect();
    assert!(
        impls.is_empty(),
        "Copy derive on non-Copy field should not generate an impl"
    );
    assert!(
        !errors.is_empty(),
        "Copy derive on non-Copy field should emit an error"
    );
}

#[test]
fn derive_eq_rejects_float_field() {
    let src = r#"
        @derive(PartialEq, Eq)
        struct Bad { x: f64 }
    "#;
    let (crate_hir, _interner, _resolved, errors) = lower_with_derives(src);
    let eq_impls: Vec<_> = crate_hir
        .items
        .values()
        .filter_map(|opt| opt.as_ref())
        .filter(|item| {
            if let ItemKind::Impl { of_trait, .. } = &item.kind {
                of_trait.is_some()
            } else {
                false
            }
        })
        .collect();
    assert!(
        eq_impls.len() <= 1,
        "Eq derive on float field should not generate an Eq impl"
    );
    assert!(
        !errors.is_empty(),
        "Eq derive on float field should emit an error"
    );
}

#[test]
fn test_attribute_on_function_ok() {
    let src = r#"
        @test
        fn my_test() {}
    "#;
    let (_crate_hir, _interner, _resolved, errors) = lower_with_derives(src);
    assert!(
        errors.is_empty(),
        "@test on a function should be accepted: {:?}",
        errors
    );
}

#[test]
fn test_attribute_on_struct_errors() {
    let src = r#"
        @test
        struct Foo {}
    "#;
    let (_crate_hir, _interner, _resolved, errors) = lower_with_derives(src);
    assert!(
        !errors.is_empty(),
        "@test on a non-function should emit an error"
    );
}

#[test]
fn ignore_without_test_errors() {
    let src = r#"
        @ignore
        fn not_a_test() {}
    "#;
    let (_crate_hir, _interner, _resolved, errors) = lower_with_derives(src);
    assert!(
        !errors.is_empty(),
        "@ignore without @test should emit an error"
    );
}

#[test]
fn repr_c_on_struct_ok() {
    let src = r#"
        @repr("C")
        struct Foo { x: i32 }
    "#;
    let (_crate_hir, _interner, _resolved, errors) = lower_with_derives(src);
    assert!(
        errors.is_empty(),
        "@repr(\"C\") on a struct should be accepted: {:?}",
        errors
    );
}

#[test]
fn packed_on_struct_ok() {
    let src = r#"
        @packed
        struct Foo { x: i32 }
    "#;
    let (_crate_hir, _interner, _resolved, errors) = lower_with_derives(src);
    assert!(
        errors.is_empty(),
        "@packed on a struct should be accepted: {:?}",
        errors
    );
}

#[test]
fn repr_c_and_packed_conflict() {
    let src = r#"
        @repr("C")
        @packed
        struct Foo { x: i32 }
    "#;
    let (_crate_hir, _interner, _resolved, errors) = lower_with_derives(src);
    assert!(
        !errors.is_empty(),
        "@repr(\"C\") and @packed together should emit an error"
    );
}

#[test]
fn unknown_repr_errors() {
    let src = r#"
        @repr("Rust")
        struct Foo { x: i32 }
    "#;
    let (_crate_hir, _interner, _resolved, errors) = lower_with_derives(src);
    assert!(
        !errors.is_empty(),
        "unknown @repr hint should emit an error"
    );
}
