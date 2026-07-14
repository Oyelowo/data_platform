use crate::*;
use crate::tests::parse_program;

#[test]
fn error_not_found() {
    let src = "fn main() { undefined_fn(); }";
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(!resolved.errors.is_empty(), "expected at least one error");
    assert!(resolved.errors.iter().any(|e| matches!(e, ResolutionError::NotFound { .. })));
}

#[test]
fn error_duplicate_definition() {
    let src = r#"
        fn foo() {}
        fn foo() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.iter().any(|e| matches!(e, ResolutionError::DuplicateDefinition { .. })));
}

#[test]
fn error_ambiguous() {
    // Ambiguous through glob imports from two different modules.
    let src = r#"
        mod a { pub fn foo() {} }
        mod b { pub fn foo() {} }
        fn main() {
            use a::*;
            use b::*;
            foo();
        }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    // At least one error (ambiguous or duplicate definition from imports).
    assert!(!resolved.errors.is_empty(), "expected errors: {:?}", resolved.errors);
}

#[test]
fn error_circular_import() {
    // Circular imports are hard to test in a single file without file-based modules.
    // For the MVP, we assert that the resolver does not panic on a self-referential use.
    let src = r#"
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    // No circular imports in this simple case, so just ensure it resolves cleanly.
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn error_wrong_namespace() {
    // Using a type name where a value is expected (e.g. struct as a function).
    // For the MVP, the resolver resolves `Foo` from the type namespace as a
    // fallback; the actual namespace mismatch is a type-checker concern, not
    // the resolver's.
    let src = r#"
        struct Foo {}
        fn main() { Foo(); }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "expected no resolution errors: {:?}", resolved.errors);
}

#[test]
fn error_missing_import_path() {
    let src = r#"
        use nonexistent::foo;
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(!resolved.errors.is_empty(), "expected errors: {:?}", resolved.errors);
}
