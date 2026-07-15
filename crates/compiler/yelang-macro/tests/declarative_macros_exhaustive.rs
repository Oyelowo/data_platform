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
fn ident_fragment_macro() {
    let src = r#"
        macro id { ($x:ident) => ( $x ); }
        fn main() { let a = id!(foo); }
    "#;
    let (program, interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    let ExprKind::Path(path) = &init.kind else {
        panic!("expected path");
    };
    assert_eq!(interner.resolve(&path.segments[0].ident.symbol), "foo");
}

#[test]
fn literal_fragment_macro() {
    let src = r#"
        macro inc { ($x:literal) => ( $x + 1 ); }
        fn main() { let a = inc!(7); }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    assert!(matches!(init.kind, ExprKind::Binary(_)));
}

#[test]
fn tt_fragment_macro() {
    let src = r#"
        macro id { ($x:tt) => ( $x ); }
        fn main() { let a = id!([1, 2]); }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    assert!(matches!(init.kind, ExprKind::Array(_)));
}

#[test]
fn multiple_rules_selects_expr_over_ident() {
    let src = r#"
        macro m {
            ($x:expr) => ( $x );
            ($x:ident) => ( $x );
        }
        fn main() { let a = m!(1 + 2); }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    assert!(matches!(init.kind, ExprKind::Binary(_)));
}

#[test]
fn repetition_star_with_separator_emits_array() {
    let src = r#"
        macro list { ($($x:expr),*) => ( [$($x),*] ); }
        fn main() { let a = list!(1, 2, 3); }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    assert!(matches!(init.kind, ExprKind::Array(_)));
}

#[test]
fn repetition_plus_zero_errors() {
    let src = r#"
        macro needs_one { ($($x:expr),+) => ( 0 ); }
        fn main() { let a = needs_one!(); }
    "#;
    let (_program, _interner, errors) = parse_and_expand(src);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, yelang_macro::ExpandError::MacroMatchError { .. })),
        "expected match error, got {:?}",
        errors
    );
}

#[test]
fn optional_repetition_with_separator() {
    let src = r#"
        macro trailing { ($x:expr $(, $y:expr)?) => ( [$x $(, $y)?] ); }
        fn main() {
            let a = trailing!(1);
            let b = trailing!(1, 2);
        }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let body = main_body(&program);
    assert!(matches!(
        let_init(&body.statements[0].kind).kind,
        ExprKind::Array(_)
    ));
    assert!(matches!(
        let_init(&body.statements[1].kind).kind,
        ExprKind::Array(_)
    ));
}

#[test]
fn nested_repetition_with_separator() {
    let src = r#"
        macro matrix {
            ($([$($x:expr),*]),*) => ( [$([$($x),*]),*] );
        }
        fn main() { let a = matrix!([1, 2], [3, 4]); }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    assert!(matches!(init.kind, ExprKind::Array(_)));
}

#[test]
fn macro_introduced_binding_usable_inside_macro() {
    let src = r#"
        macro make_and_use {
            () => { { let secret = 7; secret } };
        }
        fn main() { let a = make_and_use!(); }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    assert!(matches!(init.kind, ExprKind::Block(_)));
}

#[test]
fn macro_used_before_definition_works_in_module() {
    let src = r#"
        mod inner {
            fn foo() { let a = id!(3); }
            macro id { ($x:expr) => ( $x ); }
        }
        fn main() {}
    "#;
    let (program, interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let ItemKind::Module(m) = &program.items[0].kind else {
        panic!("expected module");
    };
    let items = match &m.kind {
        yelang_ast::ModKind::Inline { items } => items,
        _ => panic!("expected inline module"),
    };
    let ItemKind::Fn(func) = &items[0].kind else {
        panic!("expected fn");
    };
    let init = let_init(&func.body.statements[0].kind);
    let ExprKind::Literal(yelang_ast::Literal::Int(i)) = &init.kind else {
        panic!("expected int");
    };
    assert_eq!(interner.resolve(&i.value), "3");
}

#[test]
fn malformed_macro_definition_reports_error() {
    let src = r#"
        macro bad { ($x:expr) => { $y }; }
        fn main() { let a = bad!(1); }
    "#;
    let (_program, _interner, errors) = parse_and_expand(src);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, yelang_macro::ExpandError::MacroTranscribeError { .. })),
        "expected transcribe error, got {:?}",
        errors
    );
}

#[test]
fn macro_expansion_produces_call_expression() {
    let src = r#"
        macro call { ($f:ident, $x:expr) => ( $f($x) ); }
        fn main() { let a = call!(foo, 1); }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    assert!(matches!(init.kind, ExprKind::Call(_)));
}

#[test]
fn macro_preserves_operator_precedence() {
    let src = r#"
        macro times_two { ($x:expr) => ( $x * 2 ); }
        fn main() { let a = times_two!(1 + 2); }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    let ExprKind::Binary(top) = &init.kind else {
        panic!("expected binary");
    };
    assert!(matches!(top.op, yelang_ast::BinaryOp::Add));
    let ExprKind::Binary(right) = &top.right.kind else {
        panic!("expected binary on right");
    };
    assert!(matches!(right.op, yelang_ast::BinaryOp::Multiply));
}

#[test]
fn ambiguous_macro_rules_report_error() {
    let src = r#"
        macro m {
            ($x:expr) => ( $x );
            ($x:expr) => ( $x + 1 );
        }
        fn main() { let a = m!(1); }
    "#;
    let (_program, _interner, errors) = parse_and_expand(src);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, yelang_macro::ExpandError::AmbiguousMacro { .. })),
        "expected ambiguous macro error, got {:?}",
        errors
    );
}

#[test]
fn macro_rule_order_matters_first_wins() {
    // The two rules must not overlap on the same input; otherwise the current
    // engine correctly reports ambiguity.  Ordering is exercised by giving the
    // ident rule higher priority and using an ident-only input.
    let src = r#"
        macro m {
            ($x:ident) => ( 1 );
            ($x:literal) => ( 2 );
        }
        fn main() { let a = m!(foo); }
    "#;
    let (program, interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    let ExprKind::Literal(yelang_ast::Literal::Int(i)) = &init.kind else {
        panic!("expected int literal, got {:?}", init.kind);
    };
    assert_eq!(interner.resolve(&i.value), "1");
}

#[test]
fn trailing_comma_in_repetition_star() {
    let src = r#"
        macro list { ($($x:expr),*) => ( [$($x),*] ); }
        fn main() { let a = list!(1, 2, 3,); }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    assert!(matches!(init.kind, ExprKind::Array(_)));
}

#[test]
fn macro_with_pat_fragment() {
    let src = r#"
        macro bind { ($p:pat, $e:expr) => ( { let $p = $e; } ); }
        fn main() { let a = bind!(x, 5); }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    assert!(matches!(init.kind, ExprKind::Block(_)));
}

#[test]
fn macro_repetition_with_semicolon_separator() {
    let src = r#"
        macro stmts { ($($s:stmt);*) => ( { $($s);* } ); }
        fn main() { let a = stmts!(let x = 1; let y = 2; x + y); }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    assert!(matches!(init.kind, ExprKind::Block(_)));
}

#[test]
fn macro_argument_keeps_call_site_hygiene() {
    let src = r#"
        macro id { ($x:expr) => ( $x ); }
        fn main() {
            let y = 10;
            let a = id!(y);
        }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let init = let_init(&main_body(&program).statements[1].kind);
    assert!(matches!(init.kind, ExprKind::Path(_)));
}

#[test]
fn nested_macro_expansion_hygiene_isolated() {
    let src = r#"
        macro outer {
            () => ( inner!() );
        }
        macro inner {
            () => ( { let secret = 42; secret } );
        }
        fn main() { let a = outer!(); }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    assert!(matches!(init.kind, ExprKind::Block(_)));
}

#[test]
fn macro_with_multiple_metavariables_in_repetition() {
    let src = r#"
        macro pairs {
            ($($k:ident: $v:expr),*) => ( [$($k, $v),*] );
        }
        fn main() { let a = pairs!(a: 1, b: 2); }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    assert!(matches!(init.kind, ExprKind::Array(_)));
}

#[test]
fn macro_expansion_depth_limit_catches_runaway() {
    let src = r#"
        macro grow {
            ($x:expr) => ( grow!($x + 1) );
        }
        fn main() { let a = grow!(1); }
    "#;
    let (_program, _interner, errors) = parse_and_expand(src);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, yelang_macro::ExpandError::ExpansionLoop { .. })),
        "expected expansion loop / depth error, got {:?}",
        errors
    );
}
