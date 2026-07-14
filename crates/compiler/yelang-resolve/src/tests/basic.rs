use crate::*;
use crate::tests::parse_program;

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
