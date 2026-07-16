use yelang_ast::{ExprKind, ItemKind, StmtKind};
use yelang_interner::Interner;
use yelang_macro::expand_program;

fn parse_and_expand(
    src: &str,
) -> (
    yelang_ast::Program,
    Interner,
    Vec<yelang_macro::ExpandError>,
) {
    let mut interner = Interner::new();
    let mut stream = yelang_ast::TokenKind::tokenize(src, &mut interner).unwrap();
    let program = stream.parse::<yelang_ast::Program>().unwrap();
    let result = expand_program(&program, &interner);
    (result.program, interner, result.errors)
}

fn main_body(program: &yelang_ast::Program) -> &yelang_ast::BlockExpr {
    let item = program
        .items
        .iter()
        .find(|i| matches!(i.kind, ItemKind::Fn(_)))
        .expect("expected fn main");
    let ItemKind::Fn(func) = &item.kind else {
        unreachable!()
    };
    &func.body
}

fn let_init<'a>(stmt: &'a StmtKind) -> &'a yelang_ast::Expr {
    match stmt {
        StmtKind::Let(l) => l.init.as_deref().expect("let has init"),
        _ => panic!("expected let statement"),
    }
}

// ---------------------------------------------------------------------------
// Fragment fields
// ---------------------------------------------------------------------------

#[test]
fn ident_fragment_field_name() {
    let src = r#"
        macro make_const {
            ($field:ident) => { const $field.name: i32 = 0; };
        }
        make_const!(foo)
        fn main() {}
    "#;
    let (program, interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let const_item = program
        .items
        .iter()
        .find(|i| matches!(i.kind, ItemKind::Const(_)))
        .expect("const");
    let ItemKind::Const(c) = &const_item.kind else {
        unreachable!()
    };
    assert_eq!(interner.resolve(&c.name.symbol), "foo");
}

#[test]
fn expr_fragment_field_type() {
    let src = r#"
        macro ty_of {
            ($e:expr) => { $e.type };
        }
        fn main() { let t: ty_of!(x: i32) = 0; }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let stmt = &main_body(&program).statements[0];
    let StmtKind::Let(l) = &stmt.kind else {
        panic!("expected let")
    };
    let ty = l.ty.as_ref().expect("has type annotation");
    // The expanded type should be `i32`.
    assert!(matches!(ty.kind, yelang_ast::TypeKind::Named(_)));
}

#[test]
fn ty_fragment_field_name_and_args() {
    let src = r#"
        macro base_name {
            ($t:ty) => { $t.name };
        }
        macro reconstruct {
            ($t:ty) => { $t.name$t.args };
        }
        fn main() {
            let _: base_name!(Vec<i32>) = 0;
            let _: reconstruct!(Vec<i32>) = 0;
        }
    "#;
    let (program, interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let stmts = &main_body(&program).statements;
    let first_ty = match &stmts[0].kind {
        StmtKind::Let(l) => l.ty.as_ref().unwrap(),
        _ => panic!("expected let"),
    };
    let second_ty = match &stmts[1].kind {
        StmtKind::Let(l) => l.ty.as_ref().unwrap(),
        _ => panic!("expected let"),
    };
    let yelang_ast::TypeKind::Named(path) = &first_ty.kind else {
        panic!("expected named type, got {:?}", first_ty.kind);
    };
    assert_eq!(interner.resolve(&path.segments[0].ident.symbol), "Vec");
    let yelang_ast::TypeKind::Named(path2) = &second_ty.kind else {
        panic!(
            "expected named type for reconstructed, got {:?}",
            second_ty.kind
        );
    };
    assert_eq!(interner.resolve(&path2.segments[0].ident.symbol), "Vec");
    assert!(path2.segments[0].args.is_some());
}

#[test]
fn item_fragment_field_vis_and_name() {
    let src = r#"
        macro vis_name {
            ($i:item) => { $i.vis struct $i.name {} };
        }
        vis_name!(pub struct Foo { x: i32 })
        fn main() {}
    "#;
    let (program, interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let struct_item = program
        .items
        .iter()
        .find(|i| matches!(i.kind, ItemKind::Struct(_)))
        .expect("struct");
    let ItemKind::Struct(s) = &struct_item.kind else {
        unreachable!()
    };
    assert_eq!(interner.resolve(&s.name.symbol), "Foo");
    assert!(
        !struct_item.visibility.is_private(),
        "expected public visibility"
    );
}

#[test]
fn fragment_field_error_on_incompatible_accessor() {
    let src = r#"
        macro bad {
            ($x:tt) => { $x.name };
        }
        fn main() { let _ = bad!(foo); }
    "#;
    let (_program, _interner, errors) = parse_and_expand(src);
    assert!(
        !errors.is_empty(),
        "expected error for incompatible fragment field"
    );
}

#[test]
fn fragment_field_error_on_missing_component() {
    let src = r#"
        macro no_type {
            ($e:expr) => { $e.type };
        }
        fn main() { let _ = no_type!(42); }
    "#;
    let (_program, _interner, errors) = parse_and_expand(src);
    assert!(
        !errors.is_empty(),
        "expected error for missing type component"
    );
}

// ---------------------------------------------------------------------------
// Unsafe attribute and derive rules
// ---------------------------------------------------------------------------

#[test]
fn unsafe_derive_rule_invoked_with_unsafe_wrapper() {
    let src = r#"
        macro FromBytes {
            unsafe derive()(struct $name:ident $_:tt) => {
                impl $name {
                    const FROM_BYTES: bool = true;
                }
            };
        }
        @derive(unsafe(FromBytes))
        struct Packet { x: i32 }
        fn main() {}
    "#;
    let (program, interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let impl_item = program
        .items
        .iter()
        .find(|i| matches!(i.kind, ItemKind::Impl(_)))
        .expect("impl");
    let ItemKind::Impl(impl_block) = &impl_item.kind else {
        unreachable!()
    };
    assert_eq!(impl_block.items.len(), 1);
    let constant = &impl_block.items[0];
    let yelang_ast::ImplItemKind::Constant(c) = &constant.item else {
        panic!("expected constant");
    };
    assert_eq!(interner.resolve(&c.name.symbol), "FROM_BYTES");
}

#[test]
fn unsafe_derive_rule_requires_unsafe_wrapper() {
    let src = r#"
        macro FromBytes {
            unsafe derive()(struct $name:ident $_:tt) => {
                impl $name { const FROM_BYTES: bool = true; }
            };
        }
        @derive(FromBytes)
        struct Packet { x: i32 }
        fn main() {}
    "#;
    let (_program, _interner, errors) = parse_and_expand(src);
    assert!(
        !errors.is_empty(),
        "expected error for missing unsafe wrapper"
    );
    let msg = format!("{:?}", errors);
    assert!(
        msg.contains("unsafe") || msg.contains("unsafe(FromBytes)"),
        "error should mention unsafe syntax: {}",
        msg
    );
}

#[test]
fn unsafe_attr_rule_invoked_with_unsafe_wrapper() {
    let src = r#"
        macro replace_name {
            unsafe attr($new:ident)($item:item) => {
                struct $new {}
            };
        }
        @unsafe(replace_name(Baz))
        struct Foo { x: i32 }
        fn main() {}
    "#;
    let (program, interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let struct_item = program
        .items
        .iter()
        .find(|i| matches!(i.kind, ItemKind::Struct(_)))
        .expect("struct");
    let ItemKind::Struct(s) = &struct_item.kind else {
        unreachable!()
    };
    assert_eq!(interner.resolve(&s.name.symbol), "Baz");
}

#[test]
fn unsafe_attr_rule_requires_unsafe_wrapper() {
    let src = r#"
        macro replace_name {
            unsafe attr($new:ident)($item:item) => {
                struct $new {}
            };
        }
        @replace_name(Baz)
        struct Foo { x: i32 }
        fn main() {}
    "#;
    let (_program, _interner, errors) = parse_and_expand(src);
    assert!(
        !errors.is_empty(),
        "expected error for missing unsafe wrapper"
    );
    let msg = format!("{:?}", errors);
    assert!(
        msg.contains("unsafe") || msg.contains("@unsafe"),
        "error should mention unsafe syntax: {}",
        msg
    );
}

#[test]
fn unsafe_wrapper_on_safe_attr_warns_but_expands() {
    let src = r#"
        macro passthrough {
            attr()($item:item) => { $item };
        }
        @unsafe(passthrough)
        struct Foo { x: i32 }
        fn main() {}
    "#;
    let (program, interner, errors) = parse_and_expand(src);
    // The macro should expand (struct Foo kept).
    let struct_item = program
        .items
        .iter()
        .find(|i| matches!(i.kind, ItemKind::Struct(_)))
        .expect("struct");
    let ItemKind::Struct(s) = &struct_item.kind else {
        unreachable!()
    };
    assert_eq!(interner.resolve(&s.name.symbol), "Foo");
    // We expect a warning-style error about unnecessary unsafe wrapper.
    assert!(
        errors
            .iter()
            .any(|e| format!("{:?}", e).contains("unnecessary")),
        "expected unnecessary-unsafe warning: {:?}",
        errors
    );
}

// ---------------------------------------------------------------------------
// Field-position macro expansion
// ---------------------------------------------------------------------------

#[test]
fn struct_literal_field_value_macro() {
    let src = r#"
        macro val {
            () => { 42 };
        }
        struct Foo { x: i32 }
        fn main() {
            let f = Foo { x: val!() };
        }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    let ExprKind::Struct(struct_expr) = &init.kind else {
        panic!("expected struct literal, got {:?}", init.kind);
    };
    assert_eq!(struct_expr.fields.len(), 1);
    assert!(matches!(
        struct_expr.fields[0].value.kind,
        ExprKind::Literal(_)
    ));
}

#[test]
fn struct_pattern_field_value_macro() {
    let src = r#"
        macro pat {
            () => { _ };
        }
        struct Foo { x: i32 }
        fn main() {
            let Foo { x: pat!() } = Foo { x: 1 };
        }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let stmt = &main_body(&program).statements[0];
    let StmtKind::Let(l) = &stmt.kind else {
        panic!("expected let")
    };
    assert!(matches!(
        l.pattern.pattern,
        yelang_ast::PatternKind::Struct { .. }
    ));
}

#[test]
fn field_definition_type_macro() {
    let src = r#"
        macro ty {
            () => { i32 };
        }
        struct Foo { x: ty!() }
        fn main() {}
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let struct_item = program
        .items
        .iter()
        .find(|i| matches!(i.kind, ItemKind::Struct(_)))
        .expect("struct");
    let ItemKind::Struct(s) = &struct_item.kind else {
        unreachable!()
    };
    let yelang_ast::StructFields::Named(fields) = &s.fields else {
        panic!("expected named fields");
    };
    assert_eq!(fields.len(), 1);
    assert!(matches!(fields[0].ty.kind, yelang_ast::TypeKind::Named(_)));
}

#[test]
fn enum_tuple_variant_field_type_macro() {
    let src = r#"
        macro ty {
            () => { i32 };
        }
        enum Foo { Bar(ty!()) }
        fn main() {}
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let enum_item = program
        .items
        .iter()
        .find(|i| matches!(i.kind, ItemKind::Enum(_)))
        .expect("enum");
    let ItemKind::Enum(e) = &enum_item.kind else {
        unreachable!()
    };
    assert_eq!(e.variants.len(), 1);
    let yelang_ast::VariantKind::Tuple(types) = &e.variants[0].kind else {
        panic!("expected tuple variant");
    };
    assert_eq!(types.len(), 1);
    assert!(matches!(types[0].kind, yelang_ast::TypeKind::Named(_)));
}

#[test]
fn enum_struct_variant_field_type_macro() {
    let src = r#"
        macro ty {
            () => { i32 };
        }
        enum Foo { Bar { x: ty!() } }
        fn main() {}
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let enum_item = program
        .items
        .iter()
        .find(|i| matches!(i.kind, ItemKind::Enum(_)))
        .expect("enum");
    let ItemKind::Enum(e) = &enum_item.kind else {
        unreachable!()
    };
    assert_eq!(e.variants.len(), 1);
    let yelang_ast::VariantKind::Struct(fields) = &e.variants[0].kind else {
        panic!("expected struct variant");
    };
    assert_eq!(fields.len(), 1);
    assert!(matches!(fields[0].ty.kind, yelang_ast::TypeKind::Named(_)));
}

#[test]
fn array_type_size_expression_macro() {
    let src = r#"
        macro size {
            () => { 4 };
        }
        fn main() {
            let _: [i32; size!()] = [0, 0, 0, 0];
        }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let stmt = &main_body(&program).statements[0];
    let StmtKind::Let(l) = &stmt.kind else {
        panic!("expected let")
    };
    let ty = l.ty.as_ref().expect("has type annotation");
    let yelang_ast::TypeKind::Array(_, size_expr) = &ty.kind else {
        panic!("expected array type, got {:?}", ty.kind);
    };
    assert!(matches!(size_expr.kind, ExprKind::Literal(_)));
}

#[test]
fn generic_argument_type_macro() {
    let src = r#"
        macro ty {
            () => { i32 };
        }
        struct Foo<T> { x: T }
        fn main() {
            let _: Foo<ty!()> = Foo { x: 0 };
        }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let stmt = &main_body(&program).statements[0];
    let StmtKind::Let(l) = &stmt.kind else {
        panic!("expected let")
    };
    let ty = l.ty.as_ref().expect("has type annotation");
    let yelang_ast::TypeKind::Named(path) = &ty.kind else {
        panic!("expected named type, got {:?}", ty.kind);
    };
    assert!(path.segments[0].args.is_some());
}
