use yelang_ast::{Codegen, ExprKind, ItemKind, StmtKind};
use yelang_interner::Interner;
use yelang_lexer::ParseTokenStream;
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

fn let_init<'a>(stmt: &'a StmtKind) -> &'a yelang_ast::Expr {
    match stmt {
        StmtKind::Let(l) => l.init.as_deref().expect("let has init"),
        _ => panic!("expected let statement"),
    }
}

#[test]
fn simple_identity_macro() {
    let src = r#"
        macro id {
            ($x:expr) => ( $x );
        }
        fn main() {
            let a = id!(42);
        }
    "#;
    let (program, interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "errors: {:?}", errors);
    let body = main_body(&program);
    let init = let_init(&body.statements[0].kind);
    let ExprKind::Literal(yelang_ast::Literal::Int(i)) = &init.kind else {
        panic!("expected int literal, got {:?}", init.kind);
    };
    assert_eq!(interner.resolve(&i.value), "42");
}

#[test]
fn macro_used_before_definition() {
    let src = r#"
        fn main() {
            let a = double!(21);
        }
        macro double {
            ($x:expr) => ( $x * 2 );
        }
    "#;
    let (program, interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "errors: {:?}", errors);
    let body = main_body(&program);
    let mut buf = String::new();
    let_init(&body.statements[0].kind)
        .codegen(&mut buf, &interner)
        .unwrap();
    assert_eq!(buf, "21 * 2");
}

#[test]
fn macro_with_multiple_args() {
    let src = r#"
        macro add { ($x:expr, $y:expr) => ( $x + $y ); }
        fn main() { let a = add!(1, 2); }
    "#;
    let (program, interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "errors: {:?}", errors);
    let body = main_body(&program);
    let mut buf = String::new();
    let_init(&body.statements[0].kind)
        .codegen(&mut buf, &interner)
        .unwrap();
    assert_eq!(buf, "1 + 2");
}

#[test]
fn macro_repetition_star() {
    let src = r#"
        macro sum { ($($x:expr),*) => ( 0 $(+ $x)* ); }
        fn main() { let a = sum!(1, 2, 3); }
    "#;
    let (program, interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "errors: {:?}", errors);
    let body = main_body(&program);
    let mut buf = String::new();
    let_init(&body.statements[0].kind)
        .codegen(&mut buf, &interner)
        .unwrap();
    assert_eq!(buf, "0 + 1 + 2 + 3");
}

#[test]
fn macro_repetition_plus() {
    let src = r#"
        macro comma_list { ($($x:expr),+) => ( ($($x),+) ); }
        fn main() { let a = comma_list!(1, 2); }
    "#;
    let (program, interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "errors: {:?}", errors);
    let body = main_body(&program);
    let mut buf = String::new();
    let_init(&body.statements[0].kind)
        .codegen(&mut buf, &interner)
        .unwrap();
    assert_eq!(buf, "(1, 2)");
}

#[test]
fn macro_repetition_question() {
    let src = r#"
        macro maybe { ($x:expr $(, $y:expr)?) => ( $x $(+ $y)? ); }
        fn main() {
            let a = maybe!(1);
            let b = maybe!(1, 2);
        }
    "#;
    let (program, interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "errors: {:?}", errors);
    let body = main_body(&program);
    let mut a = String::new();
    let_init(&body.statements[0].kind)
        .codegen(&mut a, &interner)
        .unwrap();
    assert_eq!(a, "1");
    let mut b = String::new();
    let_init(&body.statements[1].kind)
        .codegen(&mut b, &interner)
        .unwrap();
    assert_eq!(b, "1 + 2");
}

#[test]
fn macro_with_block_fragment() {
    let src = r#"
        macro run { ($body:block) => ( $body ); }
        fn main() { let a = run!({ 42 }); }
    "#;
    let (program, interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "errors: {:?}", errors);
    let body = main_body(&program);
    let init = let_init(&body.statements[0].kind);
    assert!(matches!(init.kind, ExprKind::Block(_)));
    let mut buf = String::new();
    init.codegen(&mut buf, &interner).unwrap();
    assert!(
        buf.contains("42"),
        "expected block to contain 42, got {}",
        buf
    );
}

#[test]
fn macro_hygiene_introduced_binding_does_not_leak() {
    let src = r#"
        macro make_var {
            () => { { let secret = 7; } };
        }
        fn main() {
            make_var!();
            let secret = 3;
        }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "errors: {:?}", errors);
    let body = main_body(&program);
    assert_eq!(body.statements.len(), 2);
    assert!(matches!(
        body.statements[0].kind,
        StmtKind::TermExpr(_) | StmtKind::Expr(_)
    ));
    assert!(matches!(body.statements[1].kind, StmtKind::Let(_)));
}

#[test]
fn macro_expansion_loop_detected() {
    let src = r#"
        macro recurse {
            () => { recurse!() };
        }
        fn main() { recurse!(); }
    "#;
    let (_program, _interner, errors) = parse_and_expand(src);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, yelang_macro::ExpandError::ExpansionLoop { .. })),
        "expected expansion loop error, got {:?}",
        errors
    );
}

#[test]
fn unknown_user_macro_reports_error() {
    let src = r#"
        fn main() { does_not_exist!(1); }
    "#;
    let (_program, _interner, errors) = parse_and_expand(src);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, yelang_macro::ExpandError::UnknownMacro { .. })),
        "expected unknown macro error, got {:?}",
        errors
    );
}

#[test]
fn builtin_macros_still_expand() {
    let src = r#"
        fn main() { assert!(true); }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "errors: {:?}", errors);
    let body = main_body(&program);
    assert_eq!(body.statements.len(), 1);
    let stmt = &body.statements[0];
    let expr_kind = match &stmt.kind {
        StmtKind::TermExpr(e) | StmtKind::Expr(e) => &e.kind,
        _ => panic!("expected expression statement"),
    };
    assert!(
        matches!(expr_kind, ExprKind::If(_)),
        "expected assert! to expand to if"
    );
}

#[test]
fn macro_inside_module() {
    let src = r#"
        mod inner {
            macro id { ($x:expr) => ( $x ); }
            fn foo() { let a = id!(9); }
        }
        fn main() {}
    "#;
    let (program, interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "errors: {:?}", errors);
    let ItemKind::Module(m) = &program.items[0].kind else {
        panic!("expected module");
    };
    let items = match &m.kind {
        yelang_ast::ModKind::Inline { items } => items,
        _ => panic!("expected inline module"),
    };
    let ItemKind::Fn(func) = &items[0].kind else {
        panic!("expected fn in module");
    };
    let init = let_init(&func.body.statements[0].kind);
    let ExprKind::Literal(yelang_ast::Literal::Int(i)) = &init.kind else {
        panic!("expected int");
    };
    assert_eq!(interner.resolve(&i.value), "9");
}

#[test]
fn macro_no_rule_matches_errors() {
    let src = r#"
        macro only_ident { ($x:ident) => ( $x ); }
        fn main() { only_ident!(1 + 2); }
    "#;
    let (_program, _interner, errors) = parse_and_expand(src);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, yelang_macro::ExpandError::MacroMatchError { .. })),
        "expected macro match error, got {:?}",
        errors
    );
}

#[test]
fn macro_with_path_fragment() {
    let src = r#"
        macro path_of { ($p:path) => ( $p ); }
        fn main() { let a = path_of!(std::vec::Vec); }
    "#;
    let (program, interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "errors: {:?}", errors);
    let body = main_body(&program);
    let init = let_init(&body.statements[0].kind);
    assert!(matches!(init.kind, ExprKind::Path(_)));
    let mut buf = String::new();
    init.codegen(&mut buf, &interner).unwrap();
    assert_eq!(buf, "std::vec::Vec");
}

#[test]
fn macro_with_statement_fragment() {
    let src = r#"
        macro stmt { ($s:stmt) => ( { $s } ); }
        fn main() { stmt!(let x = 5;); }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "errors: {:?}", errors);
    let body = main_body(&program);
    let stmt_expr = match &body.statements[0].kind {
        StmtKind::TermExpr(e) | StmtKind::Expr(e) => e,
        other => panic!("expected expression statement, got {:?}", other),
    };
    let ExprKind::Block(block) = &stmt_expr.kind else {
        panic!("expected block");
    };
    assert!(matches!(block.statements[0].kind, StmtKind::Let(_)));
}

#[test]
fn macro_definition_removed_from_output() {
    let src = r#"
        macro id { ($x:expr) => ( $x ); }
        fn main() { let a = id!(1); }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "errors: {:?}", errors);
    assert!(
        program
            .items
            .iter()
            .all(|i| !matches!(i.kind, ItemKind::MacroDef(_)))
    );
}

#[test]
fn macro_nested_repetition() {
    let src = r#"
        macro matrix {
            ($([$($x:expr),*]),*) => ( [$([$($x),*]),*] );
        }
        fn main() { let a = matrix!([1, 2], [3, 4]); }
    "#;
    let (program, interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "errors: {:?}", errors);
    let body = main_body(&program);
    let init = let_init(&body.statements[0].kind);
    let mut buf = String::new();
    init.codegen(&mut buf, &interner).unwrap();
    assert_eq!(buf, "[[1, 2], [3, 4]]");
}
