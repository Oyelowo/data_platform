use crate::tests::parse_program;
use crate::*;

#[test]
fn namespace_type_vs_value() {
    // Same name can exist in both type and value namespaces without conflict.
    let src = r#"
        type x = i32;
        fn x() {}
        fn main() { let y: x = 1; x(); }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn namespace_struct_is_type() {
    let src = r#"
        struct Point { x: i32, y: i32 }
        fn main() { let p: Point; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn namespace_fn_is_value() {
    let src = r#"
        fn foo() {}
        fn main() { foo(); }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn namespace_enum_is_type() {
    let src = r#"
        enum Color { Red, Green, Blue }
        fn main() { let c: Color; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn namespace_trait_is_type() {
    let src = r#"
        trait Show { fn show(self); }
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn namespace_module_is_type() {
    let src = r#"
        mod foo { pub fn bar() {} }
        fn main() { foo::bar(); }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}
