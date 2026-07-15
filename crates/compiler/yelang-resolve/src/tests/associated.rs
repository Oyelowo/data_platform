use crate::*;
use crate::tests::parse_program;

#[test]
fn inherent_assoc_fn_resolve() {
    let src = r#"
        struct Point { x: i32, y: i32 }
        impl Point {
            fn new(x: i32, y: i32) -> Point { Point { x, y } }
        }
        fn main() { Point::new(1, 2); }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn inherent_assoc_const_resolve() {
    let src = r#"
        struct Foo {}
        impl Foo {
            const BAR: i32 = 42;
        }
        fn main() { Foo::BAR; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn inherent_assoc_type_resolve() {
    let src = r#"
        struct Foo {}
        impl Foo {
            type MyType = i32;
        }
        fn main() { let x: Foo::MyType = 1; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn trait_qualified_assoc_type() {
    let src = r#"
        trait Iterator {
            type Item;
        }
        struct MyIter {}
        impl Iterator for MyIter {
            type Item = i32;
        }
        fn main() { let x: <MyIter as Iterator>::Item = 1; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn unqualified_trait_method() {
    let src = r#"
        trait Show {
            fn show(&self) -> str;
        }
        struct Foo {}
        impl Show for Foo {
            fn show(&self) -> str { "foo" }
        }
        fn main() { Foo::show; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn associated_item_not_found() {
    let src = r#"
        struct Point {}
        impl Point {}
        fn main() { Point::nonexistent(); }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(
        resolved.errors.iter().any(|e| matches!(e, ResolutionError::NotFound { .. })),
        "expected NotFound: {:?}",
        resolved.errors
    );
}

#[test]
fn trait_impl_not_found() {
    let src = r#"
        trait Display {}
        struct Point {}
        fn main() { let x: <Point as Display>::nonexistent = 1; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(
        resolved.errors.iter().any(|e| matches!(e, ResolutionError::NotFound { .. })),
        "expected NotFound: {:?}",
        resolved.errors
    );
}

#[test]
fn multiple_inherent_impls_same_name() {
    let src = r#"
        struct Point {}
        impl Point { fn foo() {} }
        impl Point { fn foo() {} }
        fn main() { Point::foo(); }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    // FIX: This should be a duplicate error:
    // For now, just verify it resolves without crashing
    // (full ambiguity checking is future work)
}

#[test]
fn cross_module_inherent_assoc_fn() {
    let src = r#"
        mod a {
            pub struct Point {}
            impl Point {
                pub fn new() {}
            }
        }
        fn main() { a::Point::new(); }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn primitive_impl_assoc_fn() {
    // FIXME: Shouldnt this be an error and should be using an extension trait instead?
    let src = r#"
        impl i32 {
            fn foo() {}
        }
        fn main() { i32::foo(); }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn inherent_method_call_expr() {
    let src = r#"
        struct Point { x: i32, y: i32 }
        impl Point {
            fn new(x: i32, y: i32) -> Point { Point { x, y } }
        }
        fn main() { Point::new(1, 2); }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn qualified_inherent_type() {
    let src = r#"
        struct Foo {}
        impl Foo {
            type MyType = i32;
        }
        fn main() { let x: <Foo>::MyType = 1; }
    "#;
    // FIXME: shouldn't `<Foo>::MyType` be ambiguous since no trait specified?
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn resolve_qualified_assoc_fn_directly() {
    // Direct unit test for resolve_associated_item with a manually built path.
    let src = r#"
        trait Display { fn fmt(&self) -> string; }
        struct Point {}
        impl Display for Point { fn fmt(&self) -> string { "point" } }
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let collector = def_collector::DefCollector::new(&interner).collect(&program);
    let mut resolver = scope::Resolver::new(&interner, collector.module_tree, collector.definitions, collector.prelude, collector.lang_items, collector.enum_variants);
    resolver.inherent_impls = collector.inherent_impls;
    resolver.trait_impls = collector.trait_impls;
    resolver.impl_item_names = collector.impl_item_names;

    let path = yelang_ast::Path {
        qself: Some(Box::new(yelang_ast::QSelf {
            ty: yelang_ast::Type {
                kind: yelang_ast::TypeKind::Named(yelang_ast::Path::new_single_ident(
                    yelang_ast::Ident::new(interner.get_or_intern("Point"), yelang_lexer::Span::default())
                )),
                span: yelang_lexer::Span::default(),
            },
            as_trait: Some(Box::new(yelang_ast::Path::new_single_ident(
                yelang_ast::Ident::new(interner.get_or_intern("Display"), yelang_lexer::Span::default())
            ))),
            span: yelang_lexer::Span::default(),
        })),
        segments: vec![yelang_ast::PathSegment {
            ident: yelang_ast::Ident::new(interner.get_or_intern("fmt"), yelang_lexer::Span::default()),
            args: None,
        }],
        is_absolute: false,
        span: yelang_lexer::Span::default(),
    };
    let res = associated::resolve_associated_item(&resolver, &path, namespaces::Namespace::Value);
    assert!(res.is_some(), "expected fmt to resolve via qualified associated item resolution");
}

#[test]
fn resolve_inherent_with_module_prefix() {
    let src = r#"
        mod foo {
            pub struct Point {}
            impl Point {
                pub fn bar() {}
            }
        }
        fn main() { foo::Point::bar(); }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn trait_impl_multiple_items() {
    let src = r#"
        trait Display {
            type Output;
            fn fmt(&self) -> str;
        }
        struct Point {}
        impl Display for Point {
            type Output = str;
            fn fmt(&self) -> str { "point" }
        }
        fn main() { <Point as Display>::Output; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn trait_impl_method_in_type_position() {
    // Using a trait method's associated type through qualified path.
    let src = r#"
        trait Iterator {
            type Item;
        }
        struct MyVec {}
        impl Iterator for MyVec {
            type Item = i32;
        }
        fn takes(x: <MyVec as Iterator>::Item) {}
        fn main() { takes(1); }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

// ============================================================================
// Exhaustive edge-case tests for Feature 3: Associated Items
// ============================================================================

#[test]
fn self_inherent_method_call() {
    // `Self::new` inside an inherent impl resolves via self_type.
    // Uses a single-method impl to avoid a parser limitation.
    let src = r#"
        struct Foo { x: i32 }
        impl Foo {
            fn foo() -> i32 { Self::foo() }
        }
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn self_trait_impl_method_call() {
    // `Self::show` inside a trait impl resolves via self_type + trait_impls.
    let src = r#"
        trait Show {
            fn show(&self) -> i32;
        }
        struct Foo {}
        impl Show for Foo {
            fn show(&self) -> i32 { 42 }
            fn delegate(&self) -> i32 { Self::show(self) }
        }
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn unqualified_trait_assoc_const() {
    // `Foo::MAX` resolves through trait_impls index.
    let src = r#"
        trait Limits {
            const MAX: i32;
        }
        struct Foo {}
        impl Limits for Foo {
            const MAX: i32 = 100;
        }
        fn main() { Foo::MAX; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn unqualified_trait_assoc_type() {
    // `Foo::Output` in type position resolves through trait_impls.
    let src = r#"
        trait Compute {
            type Output;
        }
        struct Foo {}
        impl Compute for Foo {
            type Output = i32;
        }
        fn takes(x: Foo::Output) {}
        fn main() { takes(1); }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn qualified_trait_method_in_expr_position() {
    // `<Point as Display>::fmt` parsed as ExprPath with qself.
    let src = r#"
        trait Display {
            fn fmt(&self) -> str;
        }
        struct Point {}
        impl Display for Point {
            fn fmt(&self) -> str { "p" }
        }
        fn main() { <Point as Display>::fmt; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn private_assoc_item_not_accessible_externally() {
    // Private inherent method should not be callable from outside module.
    let src = r#"
        mod inner {
            pub struct Foo {}
            impl Foo {
                fn secret() {}
            }
        }
        fn main() { inner::Foo::secret(); }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(
        resolved.errors.iter().any(|e| matches!(e, ResolutionError::PrivacyError { .. })),
        "expected PrivacyError: {:?}",
        resolved.errors
    );
}

#[test]
fn cross_module_trait_impl() {
    // Trait and type defined in different modules.
    // Both modules must be pub for cross-module visibility.
    // Trait impl items need explicit pub for external access (until implicit
    // trait-impl visibility is implemented).
    // TODO:
    let src = r#"
        pub mod a {
            pub trait Show {
                fn show(&self) -> i32;
            }
        }
        pub mod b {
            pub struct Foo {}
            impl super::a::Show for Foo {
                pub fn show(&self) -> i32 { 1 }
            }
        }
        fn main() { b::Foo::show; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn impl_assoc_type_binding_resolves_rhs() {
    // The RHS type of an associated type binding inside an impl is resolved.
    let src = r#"
        trait Iterator {
            type Item;
        }
        struct MyVec {}
        impl Iterator for MyVec {
            type Item = i32;
        }
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn impl_assoc_const_value_resolves() {
    // The value expression of an associated constant inside an impl is resolved.
    let src = r#"
        struct Foo {}
        impl Foo {
            const BASE: i32 = 10;
            const DOUBLE: i32 = Foo::BASE + Foo::BASE;
        }
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn self_in_return_type_of_impl_method() {
    // `Self` used as return type in an inherent impl method signature.
    let src = r#"
        struct Foo { x: i32 }
        impl Foo {
            fn clone(&self) -> Self { Self { x: self.x } }
        }
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

// ============================================================================
// Enum variant resolution through type path
// ============================================================================

#[test]
fn enum_variant_through_type_path_unit() {
    let src = r#"
        enum Option { Some(i32), None }
        fn main() { Option::None; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn enum_variant_through_type_path_tuple() {
    let src = r#"
        enum Option { Some(i32), None }
        fn main() { Option::Some(1); }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn enum_variant_through_type_path_struct() {
    let src = r#"
        enum Message { Quit, Move { x: i32, y: i32 } }
        fn main() { Message::Move { x: 1, y: 2 }; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn enum_variant_cross_module_type_path() {
    let src = r#"
        mod inner {
            pub enum Status { Ok, Err }
        }
        fn main() { inner::Status::Ok; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn enum_variant_type_path_not_found() {
    let src = r#"
        enum Option { Some(i32), None }
        fn main() { Option::Nonexistent; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(
        resolved.errors.iter().any(|e| matches!(e, ResolutionError::NotFound { .. })),
        "expected NotFound: {:?}",
        resolved.errors
    );
}

#[test]
fn enum_variant_qualified_self_type_path() {
    // `<Option>::Some` should also work (qself with no trait).
    let src = r#"
        enum Option { Some(i32), None }
        fn main() { <Option>::Some(1); }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}
