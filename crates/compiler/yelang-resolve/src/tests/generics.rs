use crate::*;
use crate::tests::parse_program;

#[test]
fn resolve_generic_param_in_fn() {
    let src = "fn id<T>(x: T) -> T { x }";
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn resolve_generic_param_in_struct() {
    let src = r#"
        struct Box<T> { value: T }
        fn main() { let b: Box<i32>; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn resolve_generic_param_in_enum() {
    let src = r#"
        enum Option<T> { Some(T), None }
        fn main() { let x: Option<i32>; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn resolve_generic_param_in_trait() {
    let src = r#"
        trait Show<T> { fn show(self) -> T; }
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn resolve_generic_param_in_impl() {
    let src = r#"
        struct Point<T> { x: T }
        impl<T> Point<T> { fn get_x(self) -> T { self.x } }
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn resolve_generic_param_in_type_alias() {
    let src = r#"
        type Id<T> = T;
        fn main() { let x: Id<i32> = 1; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}
