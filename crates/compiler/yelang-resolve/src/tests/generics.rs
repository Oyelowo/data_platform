use crate::tests::parse_program;
use crate::*;

// ============================================================================
// Type generic parameter tests (Phase 1-3)
// ============================================================================

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

// ============================================================================
// Const generic parameter tests (Phase 4)
// ============================================================================

#[test]
fn resolve_const_param_in_fn_array_type() {
    // Const param used in array type in function signature.
    let src = r#"
        fn foo<const N: usize>(arr: [i32; N]) -> [i32; N] { arr }
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn resolve_const_param_in_struct() {
    // Const param used in struct field type.
    let src = r#"
        struct Array<T, const N: usize> { data: [T; N] }
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn resolve_const_param_in_enum() {
    // Const param used in enum variant payload.
    let src = r#"
        enum Buffer<const N: usize> { Fixed([u8; N]), Empty }
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn resolve_const_param_in_impl() {
    // Const param on impl block, used in method signatures.
    let src = r#"
        struct Vector<T, const N: usize> { data: [T; N] }
        impl<T, const N: usize> Vector<T, N> {
            fn len(self) -> usize { N }
        }
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn resolve_const_param_in_trait() {
    // Const param in trait definition and method signature.
    let src = r#"
        trait SizedContainer<const N: usize> {
            fn capacity(self) -> usize;
        }
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn resolve_const_param_in_type_alias() {
    // Const param in type alias definition.
    let src = r#"
        type IntArray<const N: usize> = [i32; N];
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn resolve_const_param_in_expression() {
    // Const param used as a value in function body.
    let src = r#"
        fn make_arr<const N: usize>() -> [i32; N] {
            let x = N;
            [0; N]
        }
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn resolve_const_param_in_local_type() {
    // Const param used in a local variable's type annotation.
    let src = r#"
        fn foo<const N: usize>() {
            let arr: [i32; N];
        }
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn resolve_mixed_type_and_const_params() {
    // Multiple type and const params interleaved.
    let src = r#"
        struct Matrix<T, const ROWS: usize, const COLS: usize> {
            data: [[T; COLS]; ROWS]
        }
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn resolve_const_param_in_trait_impl() {
    // Const param in trait impl method.
    let src = r#"
        trait Len { fn len(self) -> usize; }
        struct Arr<T, const N: usize> { data: [T; N] }
        impl<T, const N: usize> Len for Arr<T, N> {
            fn len(self) -> usize { N }
        }
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn resolve_const_param_in_return_type() {
    // Const param used only in return type position.
    let src = r#"
        fn zeroes<const N: usize>() -> [i32; N] { [0; N] }
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn resolve_const_param_shadowing() {
    // Const param should be shadowable by local bindings.
    let src = r#"
        fn foo<const N: usize>() {
            let N = 5;
        }
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn resolve_const_param_cross_module() {
    // Const generic struct used from another module.
    let src = r#"
        pub mod a {
            pub struct Buffer<T, const N: usize> { data: [T; N] }
        }
        fn main() { let b: a::Buffer<i32, 8>; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn resolve_const_param_with_nested_arrays() {
    // Nested array types with const params.
    let src = r#"
        struct Grid<const W: usize, const H: usize> {
            cells: [[f64; W]; H]
        }
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn unresolved_const_param_reports_error() {
    // Using an undefined const generic name should produce NotFound.
    let src = r#"
        fn foo() -> [i32; M] { [0; M] }
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(
        resolved.errors.iter().any(
            |e| matches!(e, ResolutionError::NotFound { name, .. } if interner.resolve(name) == "M")
        ),
        "expected NotFound for M: {:?}",
        resolved.errors
    );
}

#[test]
fn resolve_const_param_in_fn_type_annotation() {
    // Const param used in the type of another function parameter.
    let src = r#"
        fn process<const N: usize>(input: [i32; N], output: [i32; N]) {}
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn resolve_const_param_in_tuple_struct() {
    // Const param in tuple struct payload.
    let src = r#"
        struct VecN<T, const N: usize>([T; N]);
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn resolve_const_param_in_associated_const() {
    // Const param referenced in associated constant value.
    let src = r#"
        struct Wrap<const N: usize> {}
        impl<const N: usize> Wrap<N> {
            const VAL: usize = N;
        }
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}
