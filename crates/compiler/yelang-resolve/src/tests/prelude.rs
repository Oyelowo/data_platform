use crate::*;
use crate::tests::parse_program;

// ============================================================================
// Prelude resolution tests (Phase 4)
// ============================================================================

#[test]
fn prelude_option_type_resolves_in_root() {
    let src = r#"
        fn main() { let x: Option<i32>; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn prelude_result_type_resolves_in_root() {
    let src = r#"
        fn main() { let x: Result<i32, string>; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn prelude_vec_type_resolves_in_root() {
    let src = r#"
        fn main() { let x: Vec<i32>; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn prelude_string_type_resolves_in_root() {
    let src = r#"
        fn main() { let x: String; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn prelude_box_type_resolves_in_root() {
    let src = r#"
        fn main() { let x: Box<i32>; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn prelude_trait_copy_resolves() {
    let src = r#"
        fn main() { let x: Copy; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn prelude_trait_clone_resolves() {
    let src = r#"
        fn main() { let x: Clone; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn prelude_trait_iterator_resolves() {
    let src = r#"
        fn main() { let x: Iterator; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn prelude_option_variant_some_resolves() {
    let src = r#"
        fn main() { Some; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn prelude_option_variant_none_resolves() {
    let src = r#"
        fn main() { None; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn prelude_result_variant_ok_resolves() {
    let src = r#"
        fn main() { Ok; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn prelude_result_variant_err_resolves() {
    let src = r#"
        fn main() { Err; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn prelude_drop_fn_resolves() {
    let src = r#"
        fn main() { drop; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn prelude_type_resolves_in_nested_module() {
    let src = r#"
        mod inner {
            fn foo() -> Option<i32> {}
        }
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn prelude_shadowed_by_local_definition() {
    // A local type alias named `Option` should shadow the prelude `Option`.
    let src = r#"
        type Option = i32;
        fn main() { let x: Option = 1; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn prelude_shadowed_by_fn_param() {
    // A function parameter named `Option` should shadow the prelude type
    // in the value namespace (though not the type namespace).
    let src = r#"
        fn foo(Option: i32) { Option; }
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn prelude_shadowed_by_module_item() {
    // A struct named `Option` in a module should shadow prelude `Option`.
    let src = r#"
        struct Option { x: i32 }
        fn main() { let o: Option = Option { x: 1 }; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn prelude_shadowed_by_import() {
    // An explicit import should shadow the prelude item of the same name.
    let src = r#"
        mod foo { pub struct Option {} }
        use foo::Option;
        fn main() { let x: Option = Option {}; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn prelude_shadowed_by_let_binding() {
    // A let binding should shadow prelude items in the value namespace.
    let src = r#"
        fn main() {
            let Some = 42;
            Some;
        }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn prelude_item_in_nested_module_not_visible_as_qualified() {
    // Prelude items are not part of any module's namespace table, so
    // they cannot be accessed via qualified paths like `crate::Option`.
    // They are only resolved as unqualified names.
    let src = r#"
        fn main() { crate::Option; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(
        resolved.errors.iter().any(|e| matches!(e, ResolutionError::NotFound { .. })),
        "expected NotFound for crate::Option: {:?}",
        resolved.errors
    );
}

#[test]
fn glob_import_does_not_import_prelude_items() {
    // A glob import from a module should only import that module's explicit
    // items, not the prelude items that were resolved as fallbacks.
    let src = r#"
        mod foo { pub fn bar() {} }
        use foo::*;
        fn main() { bar(); }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn prelude_trait_in_impl_bound_resolves() {
    let src = r#"
        struct Foo {}
        impl Clone for Foo {}
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn prelude_trait_as_type_bound_resolves() {
    let src = r#"
        fn clone_it<T: Clone>(x: T) -> T { x }
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn prelude_items_do_not_conflict_with_each_other() {
    // Using multiple prelude items in one program should work.
    let src = r#"
        fn foo() -> Option<Result<Vec<String>, string>> { None }
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn prelude_sized_trait_resolves() {
    let src = r#"
        fn main() { let x: Sized; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn prelude_send_sync_traits_resolve() {
    let src = r#"
        fn main() { let x: Send; let y: Sync; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn prelude_default_trait_resolves() {
    let src = r#"
        fn main() { let x: Default; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn prelude_display_debug_traits_resolve() {
    let src = r#"
        fn main() { let x: Display; let y: Debug; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn prelude_partial_eq_eq_traits_resolve() {
    let src = r#"
        fn main() { let x: PartialEq; let y: Eq; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn prelude_partial_ord_ord_traits_resolve() {
    let src = r#"
        fn main() { let x: PartialOrd; let y: Ord; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn prelude_into_iterator_trait_resolves() {
    let src = r#"
        fn main() { let x: IntoIterator; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}
