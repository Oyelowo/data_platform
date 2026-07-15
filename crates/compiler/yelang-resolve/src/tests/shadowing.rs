use crate::tests::parse_program;
use crate::*;

#[test]
fn shadow_local_with_param() {
    let src = "fn foo(x: i32) { let x = 1; x; }";
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn shadow_outer_local() {
    let src = "fn main() { let x = 1; { let x = 2; x; } }";
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn shadow_fn_name_in_local() {
    let src = "fn foo() {} fn main() { let foo = 1; foo; }";
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn pattern_binding_shadows() {
    let src = "fn main() { let x = 1; match x { y => { let y = 2; y } } }";
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn block_scope_shadowing() {
    let src = "fn main() { let x = 1; { let x = 2; { let x = 3; x; } } }";
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn for_loop_pattern_shadowing() {
    let src = "fn main() { let x = 1; for x in 0..10 { x; } }";
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}
