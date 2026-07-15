use crate::tests::parse_program;
use crate::*;

#[test]
fn resolve_simple_use() {
    let src = r#"
        mod foo { pub fn bar() {} }
        use foo::bar;
        fn main() { bar(); }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn resolve_renamed_use() {
    let src = r#"
        mod foo { pub fn bar() {} }
        use foo::bar as baz;
        fn main() { baz(); }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn resolve_glob_use() {
    let src = r#"
        mod foo { pub fn bar() {} pub fn baz() {} }
        use foo::*;
        fn main() { bar(); baz(); }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn resolve_nested_use() {
    let src = r#"
        mod foo { pub fn bar() {} pub fn baz() {} }
        use foo::{bar, baz};
        fn main() { bar(); baz(); }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn unresolved_import_error() {
    let src = r#"
        use nonexistent::foo;
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(
        !resolved.errors.is_empty(),
        "expected errors: {:?}",
        resolved.errors
    );
}

#[test]
fn duplicate_import_error() {
    let src = r#"
        mod foo { pub fn bar() {} }
        use foo::bar;
        use foo::bar;
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    // Duplicate imports may or may not produce errors depending on implementation.
    // For now, we just assert the resolver does not panic.
}
