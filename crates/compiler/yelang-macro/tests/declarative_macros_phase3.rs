use yelang_ast::{Codegen, ExprKind, ItemKind, PatternKind, StmtKind, TypeKind};
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
    let item = &program.items[0];
    let ItemKind::Fn(func) = &item.kind else {
        panic!("expected fn main");
    };
    &func.body
}

fn let_stmt<'a>(stmt: &'a StmtKind) -> &'a yelang_ast::LetStmt {
    match stmt {
        StmtKind::Let(l) => l,
        _ => panic!("expected let statement"),
    }
}

// ============================================================
// Type-position macro invocations
// ============================================================

#[test]
fn type_position_macro_expands_in_let_annotation() {
    let src = r#"
        macro MyType { () => ( i32 ); }
        fn main() { let x: MyType!() = 42; }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);

    let stmt = &main_body(&program).statements[0];
    let let_stmt = let_stmt(&stmt.kind);
    let ty = let_stmt.ty.as_deref().expect("let has type annotation");
    assert!(
        matches!(ty.kind, TypeKind::Named(_)),
        "expected expanded type, got {:?}",
        ty.kind
    );
}

#[test]
fn type_position_macro_expands_in_fn_return_type() {
    let src = r#"
        macro RetTy { () => ( i32 ); }
        fn foo() -> RetTy!() { 1 }
        fn main() {}
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);

    let item = &program.items[0];
    let ItemKind::Fn(func) = &item.kind else {
        panic!("expected fn");
    };
    let ret = match &func.sig.return_type {
        yelang_ast::FnRefType::Type(t) => t,
        _ => panic!("expected explicit return type"),
    };
    assert!(
        matches!(ret.kind, TypeKind::Named(_)),
        "expected expanded return type, got {:?}",
        ret.kind
    );
}

#[test]
fn type_position_macro_expands_in_param_type() {
    let src = r#"
        macro ArgTy { () => ( i32 ); }
        fn foo(x: ArgTy!()) -> i32 { x }
        fn main() {}
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);

    let item = &program.items[0];
    let ItemKind::Fn(func) = &item.kind else {
        panic!("expected fn");
    };
    assert!(matches!(func.sig.params[0].ty.kind, TypeKind::Named(_)));
}

#[test]
fn type_position_macro_expands_in_generic_arg() {
    let src = r#"
        macro Inner { () => ( i32 ); }
        fn main() { let x: Vec<Inner!()>; }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);

    let stmt = &main_body(&program).statements[0];
    let let_stmt = let_stmt(&stmt.kind);
    let ty = let_stmt.ty.as_deref().expect("let has type annotation");
    assert!(
        matches!(ty.kind, TypeKind::Named(_)),
        "expected expanded generic type, got {:?}",
        ty.kind
    );
}

#[test]
fn nested_type_macro_expansion() {
    let src = r#"
        macro A { () => ( B!() ); }
        macro B { () => ( i32 ); }
        fn main() { let x: A!() = 1; }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);

    let stmt = &main_body(&program).statements[0];
    let let_stmt = let_stmt(&stmt.kind);
    let ty = let_stmt.ty.as_deref().expect("let has type annotation");
    assert!(
        matches!(ty.kind, TypeKind::Named(_)),
        "expected fully expanded type, got {:?}",
        ty.kind
    );
}

// ============================================================
// Pattern-position macro invocations
// ============================================================

#[test]
fn pattern_position_macro_expands_in_let_binding() {
    let src = r#"
        macro MyPat { () => ( x ); }
        fn main() { let MyPat!() = 42; }
    "#;
    let (program, interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);

    let stmt = &main_body(&program).statements[0];
    let let_stmt = let_stmt(&stmt.kind);
    assert!(
        matches!(
            let_stmt.pattern.pattern,
            PatternKind::Binding { ref name, .. }
                if interner.resolve(&name.symbol) == "x"
        ),
        "expected binding pattern, got {:?}",
        let_stmt.pattern.pattern
    );
}

#[test]
fn pattern_position_macro_expands_in_fn_param() {
    let src = r#"
        macro MyPat { () => ( x ); }
        fn foo(MyPat!(): i32) -> i32 { x }
        fn main() {}
    "#;
    let (program, interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);

    let item = &program.items[0];
    let ItemKind::Fn(func) = &item.kind else {
        panic!("expected fn");
    };
    assert!(
        matches!(
            func.sig.params[0].pattern.pattern,
            PatternKind::Binding { ref name, .. }
                if interner.resolve(&name.symbol) == "x"
        ),
        "expected binding pattern, got {:?}",
        func.sig.params[0].pattern.pattern
    );
}

#[test]
fn pattern_position_macro_expands_tuple_pattern() {
    // The transcriber's outer delimiters are stripped, so to emit a tuple
    // pattern we wrap it in an extra group: contents are `(a, b)`.
    let src = r#"
        macro TupPat { () => ((a, b)); }
        fn main() { let TupPat!() = (1, 2); }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);

    let stmt = &main_body(&program).statements[0];
    let let_stmt = let_stmt(&stmt.kind);
    assert!(
        matches!(let_stmt.pattern.pattern, PatternKind::Tuple { .. }),
        "expected tuple pattern, got {:?}",
        let_stmt.pattern.pattern
    );
}

// ============================================================
// Item-position macro invocations
// ============================================================

#[test]
fn item_position_macro_expands_to_fn() {
    let src = r#"
        macro make_fn { () => ( fn generated() -> i32 { 42 } ); }
        make_fn!()
        fn main() {}
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);

    let fn_items: Vec<_> = program
        .items
        .iter()
        .filter(|i| matches!(i.kind, ItemKind::Fn(_)))
        .collect();
    assert_eq!(fn_items.len(), 2, "expected main and generated fn");
}

#[test]
fn item_position_macro_expands_to_multiple_items() {
    let src = r#"
        macro make_items { () => (
            fn one() -> i32 { 1 }
            fn two() -> i32 { 2 }
        ); }
        make_items!()
        fn main() {}
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);

    let fn_items: Vec<_> = program
        .items
        .iter()
        .filter(|i| matches!(i.kind, ItemKind::Fn(_)))
        .collect();
    assert_eq!(fn_items.len(), 3, "expected main, one, and two");
}

#[test]
fn item_position_macro_inside_module() {
    let src = r#"
        macro make_fn { () => ( fn inner() -> i32 { 7 } ); }
        mod outer {
            make_fn!()
        }
        fn main() {}
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);

    let mod_item = program
        .items
        .iter()
        .find(|i| matches!(i.kind, ItemKind::Module(_)))
        .expect("expected module");
    let ItemKind::Module(m) = &mod_item.kind else {
        unreachable!()
    };
    let yelang_ast::ModKind::Inline { items } = &m.kind else {
        panic!("expected inline module");
    };
    assert_eq!(items.len(), 1);
    assert!(matches!(items[0].kind, ItemKind::Fn(_)));
}

// ============================================================
// Mixed positions and edge cases
// ============================================================

#[test]
fn macro_in_type_cast_position() {
    let src = r#"
        macro T { () => ( i32 ); }
        fn main() { let x = 1 as T!(); }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);

    let stmt = &main_body(&program).statements[0];
    let let_stmt = let_stmt(&stmt.kind);
    let init = let_stmt.init.as_deref().expect("let has init");
    assert!(
        matches!(init.kind, ExprKind::TypeCast(_)),
        "expected type cast, got {:?}",
        init.kind
    );
}

#[test]
fn macro_in_match_arm_pattern() {
    let src = r#"
        macro P { () => ( x ); }
        fn main() {
            match Some(1) {
                Some(P!()) => {}
                _ => {}
            }
        }
    "#;
    let (program, interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);

    let stmt = &main_body(&program).statements[0];
    let expr = match &stmt.kind {
        StmtKind::TermExpr(e) | StmtKind::Expr(e) => e,
        _ => panic!("expected expr stmt"),
    };
    let ExprKind::Match(m) = &expr.kind else {
        panic!("expected match");
    };
    assert!(
        matches!(m.arms[0].pattern.pattern, PatternKind::TupleStruct { .. }),
        "expected tuple-struct pattern, got {:?}",
        m.arms[0].pattern.pattern
    );
    // The inner binding should be `x`.
    let mut rendered = String::new();
    m.arms[0].pattern.codegen(&mut rendered, &interner).unwrap();
    assert!(rendered.contains("x"), "expanded pattern: {}", rendered);
}

// ============================================================
// Error cases
// ============================================================

#[test]
fn unknown_macro_in_type_position_emits_error() {
    let src = r#"
        fn main() { let x: unknown!() = 1; }
    "#;
    let (_program, _interner, errors) = parse_and_expand(src);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, yelang_macro::ExpandError::UnknownMacro { .. })),
        "expected UnknownMacro error, got {:?}",
        errors
    );
}

#[test]
fn macro_producing_invalid_type_emits_error() {
    let src = r#"
        macro Bad { () => ( 1 + 2 ); }
        fn main() { let x: Bad!() = 1; }
    "#;
    let (_program, _interner, errors) = parse_and_expand(src);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, yelang_macro::ExpandError::MalformedMacroArgs { .. })),
        "expected MalformedMacroArgs error, got {:?}",
        errors
    );
}

#[test]
fn macro_producing_invalid_pattern_emits_error() {
    let src = r#"
        macro Bad { () => ( 1 + 2 ); }
        fn main() { let Bad!() = 1; }
    "#;
    let (_program, _interner, errors) = parse_and_expand(src);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, yelang_macro::ExpandError::MalformedMacroArgs { .. })),
        "expected MalformedMacroArgs error, got {:?}",
        errors
    );
}

#[test]
fn macro_producing_invalid_item_emits_error() {
    let src = r#"
        macro Bad { () => ( 1 + 2 ); }
        Bad!()
        fn main() {}
    "#;
    let (_program, _interner, errors) = parse_and_expand(src);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, yelang_macro::ExpandError::MalformedMacroArgs { .. })),
        "expected MalformedMacroArgs error, got {:?}",
        errors
    );
}
