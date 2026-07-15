use yelang_ast::{ExprKind, ItemKind, PatternKind, StmtKind};
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
        _ => panic!("expected let statement, got {:?}", stmt),
    }
}

fn term_or_expr<'a>(stmt: &'a StmtKind) -> &'a yelang_ast::Expr {
    match stmt {
        StmtKind::Expr(e) | StmtKind::TermExpr(e) => e,
        _ => panic!("expected expression statement, got {:?}", stmt),
    }
}

fn let_init<'a>(stmt: &'a StmtKind) -> &'a yelang_ast::Expr {
    let_stmt(stmt).init.as_deref().expect("let has init")
}

// ============================================================
// Statement-position macro invocations
// ============================================================

#[test]
fn stmt_macro_expands_to_let_binding() {
    let src = r#"
        macro make_let {
            ($name:ident, $value:expr) => {
                let $name = $value;
            };
        }
        fn main() {
            make_let! { x, 42 }
            let y = x;
        }
    "#;
    let (program, interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "errors: {:?}", errors);

    let body = main_body(&program);
    assert_eq!(body.statements.len(), 2, "expected let from macro + use");
    let first = let_stmt(&body.statements[0].kind);
    let init = first.init.as_deref().unwrap();
    let ExprKind::Literal(yelang_ast::Literal::Int(i)) = &init.kind else {
        panic!("expected int literal");
    };
    assert_eq!(interner.resolve(&i.value), "42");
    let PatternKind::Binding { name, .. } = &first.pattern.pattern else {
        panic!("expected binding pattern");
    };
    assert_eq!(interner.resolve(&name.symbol), "x");
}

#[test]
fn stmt_macro_expands_to_expression_statement() {
    let src = r#"
        macro emit_expr {
            () => { 42; };
        }
        fn main() {
            emit_expr! {}
        }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "errors: {:?}", errors);

    let body = main_body(&program);
    assert_eq!(body.statements.len(), 1);
    assert!(
        matches!(body.statements[0].kind, StmtKind::TermExpr(_)),
        "expected discarded expression statement"
    );
}

#[test]
fn stmt_macro_expands_to_block_local_item() {
    let src = r#"
        macro make_fn {
            () => {
                fn helper() -> i32 { 7 }
            };
        }
        fn main() {
            make_fn! {}
            let x = helper();
        }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "errors: {:?}", errors);

    let body = main_body(&program);
    assert_eq!(body.statements.len(), 2, "expected item + let");
    assert!(matches!(body.statements[0].kind, StmtKind::Item(_)));
    assert!(matches!(body.statements[1].kind, StmtKind::Let(_)));
}

#[test]
fn stmt_macro_expands_to_multiple_statements() {
    let src = r#"
        macro two_lets {
            () => {
                let a = 1;
                let b = 2;
            };
        }
        fn main() {
            two_lets! {}
            let s = a + b;
        }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "errors: {:?}", errors);

    let body = main_body(&program);
    assert_eq!(body.statements.len(), 3, "expected two lets + use");
    assert!(matches!(body.statements[0].kind, StmtKind::Let(_)));
    assert!(matches!(body.statements[1].kind, StmtKind::Let(_)));
    assert!(matches!(body.statements[2].kind, StmtKind::Let(_)));
}

#[test]
fn stmt_macro_inside_if_body() {
    let src = r#"
        macro inc {
            ($name:ident) => {
                $name = $name + 1;
            };
        }
        fn main() {
            let mut x = 0;
            if true {
                inc! { x }
            }
        }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "errors: {:?}", errors);

    let body = main_body(&program);
    let if_expr = term_or_expr(&body.statements[1].kind);
    let ExprKind::If(if_) = &if_expr.kind else {
        panic!("expected if");
    };
    assert_eq!(if_.then_block.statements.len(), 1);
    assert!(matches!(
        if_.then_block.statements[0].kind,
        StmtKind::TermExpr(_)
    ));
}

#[test]
fn stmt_macro_inside_match_arm_body() {
    let src = r#"
        macro inc {
            ($name:ident) => {
                $name = $name + 1;
            };
        }
        fn main() {
            let mut x = 0;
            match true {
                true => {
                    inc! { x }
                }
            }
        }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "errors: {:?}", errors);

    let body = main_body(&program);
    let match_expr = term_or_expr(&body.statements[1].kind);
    let ExprKind::Match(match_) = &match_expr.kind else {
        panic!("expected match");
    };
    assert_eq!(match_.arms.len(), 1);
    let ExprKind::Block(arm_block) = &match_.arms[0].body.kind else {
        panic!("expected block arm body");
    };
    assert_eq!(arm_block.statements.len(), 1);
    assert!(matches!(
        arm_block.statements[0].kind,
        StmtKind::TermExpr(_)
    ));
}

#[test]
fn stmt_macro_inside_loop_body() {
    let src = r#"
        macro inc {
            ($name:ident) => {
                $name = $name + 1;
            };
        }
        fn main() {
            let mut x = 0;
            loop {
                inc! { x }
                break;
            }
        }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "errors: {:?}", errors);

    let body = main_body(&program);
    let loop_expr = term_or_expr(&body.statements[1].kind);
    let ExprKind::Loop(loop_) = &loop_expr.kind else {
        panic!("expected loop");
    };
    assert_eq!(loop_.body.statements.len(), 2);
    assert!(matches!(
        loop_.body.statements[0].kind,
        StmtKind::TermExpr(_)
    ));
    assert!(matches!(
        loop_.body.statements[1].kind,
        StmtKind::TermExpr(_)
    ));
}

#[test]
fn stmt_macro_inside_closure_body() {
    let src = r#"
        macro inc {
            ($name:ident) => {
                $name = $name + 1;
            };
        }
        fn main() {
            let mut x = 0;
            let f = || {
                inc! { x }
            };
        }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "errors: {:?}", errors);

    let body = main_body(&program);
    let init = let_init(&body.statements[1].kind);
    let ExprKind::Lambda(lambda) = &init.kind else {
        panic!("expected lambda");
    };
    let ExprKind::Block(block) = &lambda.body.kind else {
        panic!("expected block body");
    };
    assert_eq!(block.statements.len(), 1);
    assert!(matches!(block.statements[0].kind, StmtKind::TermExpr(_)));
}

#[test]
fn nested_stmt_macros_expand() {
    let src = r#"
        macro inner {
            () => {
                let y = 2;
            };
        }
        macro outer {
            () => {
                inner! {}
                let x = 1;
            };
        }
        fn main() {
            outer! {}
        }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "errors: {:?}", errors);

    let body = main_body(&program);
    assert_eq!(body.statements.len(), 2, "expected inner let + outer let");
    assert!(matches!(body.statements[0].kind, StmtKind::Let(_)));
    assert!(matches!(body.statements[1].kind, StmtKind::Let(_)));
}

#[test]
fn delimiter_rule_brace_is_stmt_macro() {
    let src = r#"
        macro stmt_expr {
            () => { 42 };
        }
        fn main() {
            stmt_expr! {}
            let a = stmt_expr!();
        }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "errors: {:?}", errors);

    let body = main_body(&program);
    // `stmt_expr! {}` is a statement macro: expands to terminating expr `42`.
    assert!(matches!(body.statements[0].kind, StmtKind::Expr(_)));
    // `stmt_expr!()` is an expression macro used as a let initializer.
    assert!(matches!(body.statements[1].kind, StmtKind::Let(_)));
}

#[test]
fn delimiter_rule_parens_and_brackets_are_expr_macros() {
    let src = r#"
        macro value {
            () => { 99 };
        }
        fn main() {
            let a = value!();
            let b = value![];
        }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "errors: {:?}", errors);

    let body = main_body(&program);
    assert_eq!(body.statements.len(), 2);
    let a_init = let_init(&body.statements[0].kind);
    assert!(matches!(a_init.kind, ExprKind::Literal(_)));
    let b_init = let_init(&body.statements[1].kind);
    assert!(matches!(b_init.kind, ExprKind::Literal(_)));
}

#[test]
fn stmt_macro_preserves_semicolon_terminated_output() {
    let src = r#"
        macro discard {
            ($e:expr) => { $e; };
        }
        fn main() {
            discard! { 42 }
            let x = 1;
        }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "errors: {:?}", errors);

    let body = main_body(&program);
    assert_eq!(body.statements.len(), 2);
    assert!(
        matches!(body.statements[0].kind, StmtKind::TermExpr(_)),
        "expected discarded expression"
    );
}

// ============================================================
// Error cases
// ============================================================

#[test]
fn unknown_stmt_macro_reports_error() {
    let src = r#"
        fn main() {
            does_not_exist! {}
        }
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
fn stmt_macro_producing_invalid_statement_errors() {
    let src = r#"
        macro bad {
            () => { let };
        }
        fn main() {
            bad! {}
        }
    "#;
    let (_program, _interner, errors) = parse_and_expand(src);
    assert!(
        !errors.is_empty(),
        "expected malformed macro expansion error, got {:?}",
        errors
    );
}

#[test]
fn expr_macro_at_statement_position_must_produce_expression() {
    // `maybe_let!()` is parsed as an expression statement, so its expansion
    // must be a valid expression. Producing a `let` statement is an error.
    let src = r#"
        macro maybe_let {
            () => { let x = 1; };
        }
        fn main() {
            maybe_let!();
        }
    "#;
    let (_program, _interner, errors) = parse_and_expand(src);
    assert!(
        !errors.is_empty(),
        "expected error for expression macro producing statement, got {:?}",
        errors
    );
}

#[test]
fn stmt_macro_expanding_to_empty_is_allowed() {
    let src = r#"
        macro noop {
            () => {};
        }
        fn main() {
            noop! {}
            let x = 1;
        }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "errors: {:?}", errors);

    let body = main_body(&program);
    assert_eq!(body.statements.len(), 1);
    assert!(matches!(body.statements[0].kind, StmtKind::Let(_)));
}

#[test]
fn stmt_macro_can_expand_to_block_expression() {
    let src = r#"
        macro make_block {
            () => { { 42 } };
        }
        fn main() {
            make_block! {}
        }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "errors: {:?}", errors);

    let body = main_body(&program);
    assert_eq!(body.statements.len(), 1);
    let expr = term_or_expr(&body.statements[0].kind);
    assert!(
        matches!(expr.kind, ExprKind::Block(_)),
        "expected block expression, got {:?}",
        expr.kind
    );
}
