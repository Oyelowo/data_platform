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
    let mut resolver = scope::Resolver::new(&interner, collector.module_tree, collector.definitions);
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
