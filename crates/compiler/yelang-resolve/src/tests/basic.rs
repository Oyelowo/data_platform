use crate::tests::parse_program;
use crate::*;

#[test]
fn resolve_fn_call() {
    let src = "fn foo() {} fn main() { foo(); }";
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn resolve_struct_literal() {
    let src = r#"
        struct Point { x: i32, y: i32 }
        fn main() { Point { x: 1, y: 2 }; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn resolve_local_variable() {
    let src = "fn main() { let x = 1; x; }";
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn resolve_module_path() {
    let src = r#"
        mod foo { pub fn bar() {} }
        fn main() { foo::bar(); }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn resolve_type_alias() {
    let src = r#"
        type Int = i32;
        fn main() { let x: Int = 1; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn resolve_enum_variant() {
    let src = r#"
        enum Option { Some(i32), None }
        fn main() { Some(1); }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

// ============================================================================
// Block item hoisting
// ============================================================================

#[test]
fn block_fn_forward_reference() {
    // `bar()` is called before `fn bar()` is declared inside the block.
    let src = r#"
        fn foo() {
            bar();
            fn bar() {}
        }
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn block_struct_forward_reference() {
    let src = r#"
        fn foo() {
            let x: Bar;
            struct Bar {}
        }
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn block_enum_forward_reference() {
    let src = r#"
        fn foo() {
            let x: Color;
            enum Color { Red, Green }
        }
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn block_enum_variant_forward_reference() {
    // Enum variants should be hoisted along with the enum.
    let src = r#"
        fn foo() {
            let x = Red;
            enum Color { Red, Green }
        }
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn block_const_forward_reference() {
    let src = r#"
        fn foo() {
            let x = BASE;
            const BASE: i32 = 10;
        }
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn block_static_forward_reference() {
    let src = r#"
        fn foo() {
            let x = BASE;
            static BASE: i32 = 10;
        }
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn block_type_alias_forward_reference() {
    let src = r#"
        fn foo() {
            let x: MyInt = 1;
            type MyInt = i32;
        }
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn block_trait_forward_reference() {
    let src = r#"
        fn foo() {
            fn takes(x: impl Show) {}
            trait Show { fn show(&self); }
        }
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

// ============================================================================
// Self in struct literals inside impls
// ============================================================================

#[test]
fn self_struct_literal_inherent_impl() {
    let src = r#"
        struct Foo { x: i32 }
        impl Foo {
            fn new(x: i32) -> Self { Self { x } }
        }
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn self_struct_literal_trait_impl() {
    let src = r#"
        trait Clone { fn clone(&self) -> Self; }
        struct Foo { x: i32 }
        impl Clone for Foo {
            fn clone(&self) -> Self { Self { x: self.x } }
        }
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}
