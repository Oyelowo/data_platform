use yelang_ast::{ExprKind, ItemKind, StmtKind};
use yelang_interner::Interner;
use yelang_macro::{ExpandError, expand_program};

fn parse_and_expand(src: &str) -> (yelang_ast::Program, Interner, Vec<ExpandError>) {
    let mut interner = Interner::new();
    let mut stream = yelang_ast::TokenKind::tokenize(src, &mut interner).unwrap();
    let program = stream.parse::<yelang_ast::Program>().unwrap();
    let result = expand_program(&program, &interner);
    (result.program, interner, result.errors)
}

fn main_body<'a>(
    program: &'a yelang_ast::Program,
    interner: &Interner,
) -> &'a yelang_ast::BlockExpr {
    let item = program
        .items
        .iter()
        .find(|i| {
            let ItemKind::Fn(func) = &i.kind else {
                return false;
            };
            interner.resolve(&func.name.symbol) == "main"
        })
        .expect("expected fn main");
    let ItemKind::Fn(func) = &item.kind else {
        unreachable!();
    };
    &func.body
}

fn first_stmt_expr<'a>(
    program: &'a yelang_ast::Program,
    interner: &Interner,
) -> &'a yelang_ast::Expr {
    let stmt = &main_body(program, interner).statements[0];
    match &stmt.kind {
        StmtKind::Expr(e) | StmtKind::TermExpr(e) => e,
        _ => panic!("expected expression statement"),
    }
}

// ============================================================
// Expansion-loop / recursion detection
// ============================================================

#[test]
fn direct_recursion_is_detected() {
    let src = r#"
        macro bomb { () => { bomb!() }; }
        fn main() { bomb!(); }
    "#;
    let (_program, _interner, errors) = parse_and_expand(src);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, ExpandError::ExpansionLoop { .. })),
        "expected expansion-loop error, got {:?}",
        errors
    );
}

#[test]
fn indirect_recursion_is_detected() {
    let src = r#"
        macro a { () => { b!() }; }
        macro b { () => { a!() }; }
        fn main() { a!(); }
    "#;
    let (_program, _interner, errors) = parse_and_expand(src);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, ExpandError::ExpansionLoop { .. })),
        "expected expansion-loop error for indirect recursion, got {:?}",
        errors
    );
}

// ============================================================
// Diagnostic backtraces
// ============================================================

#[test]
fn expansion_loop_includes_backtrace() {
    let src = r#"
        macro a { () => { b!() }; }
        macro b { () => { a!() }; }
        fn main() { a!(); }
    "#;
    let (_program, _interner, errors) = parse_and_expand(src);
    let loop_err = errors
        .iter()
        .find(|e| matches!(e, ExpandError::ExpansionLoop { .. }))
        .expect("expected expansion-loop error");
    let bt = match loop_err {
        ExpandError::ExpansionLoop { backtrace, .. } => backtrace,
        _ => unreachable!(),
    };
    assert!(
        bt.iter().any(|f| f.name == "a"),
        "backtrace should include macro a: {:?}",
        bt
    );
    assert!(
        bt.iter().any(|f| f.name == "b"),
        "backtrace should include macro b: {:?}",
        bt
    );
}

#[test]
fn unknown_macro_error_includes_backtrace() {
    let src = r#"
        macro outer { () => { inner!() }; }
        fn main() { outer!(); }
    "#;
    let (_program, _interner, errors) = parse_and_expand(src);
    let unknown = errors
        .iter()
        .find(|e| matches!(e, ExpandError::UnknownMacro { .. }))
        .expect("expected unknown-macro error");
    let bt = match unknown {
        ExpandError::UnknownMacro { backtrace, .. } => backtrace,
        _ => unreachable!(),
    };
    assert!(
        bt.iter().any(|f| f.name == "outer"),
        "backtrace should include outer macro: {:?}",
        bt
    );
}

// ============================================================
// $crate hygiene
// ============================================================

#[test]
fn dollar_crate_expands_to_crate_anchored_path() {
    let src = r#"
        macro use_foo { () => { $crate::foo() }; }
        fn foo() -> i32 { 1 }
        fn main() { use_foo!(); }
    "#;
    let (program, interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);

    let expr = first_stmt_expr(&program, &interner);
    let ExprKind::Call(call) = &expr.kind else {
        panic!("expected call expression, got {:?}", expr.kind);
    };
    let ExprKind::Path(path) = &call.callee.kind else {
        panic!("expected path callee, got {:?}", call.callee.kind);
    };
    assert_eq!(path.segments.len(), 2, "expected $crate::foo");
    let first = &path.segments[0].ident;
    assert_eq!(first.as_str(&interner), "crate");
    assert_eq!(
        first.origin,
        yelang_ast::tokenizer::IdentOrigin::Crate,
        "first segment should have Crate origin"
    );
    let second = &path.segments[1].ident;
    assert_eq!(second.as_str(&interner), "foo");
}

#[test]
fn dollar_package_expands_to_package_anchored_path() {
    let src = r#"
        macro use_pkg { () => { $package::bar() }; }
        fn main() { use_pkg!(); }
    "#;
    let (program, interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);

    let expr = first_stmt_expr(&program, &interner);
    let ExprKind::Call(call) = &expr.kind else {
        panic!("expected call expression, got {:?}", expr.kind);
    };
    let ExprKind::Path(path) = &call.callee.kind else {
        panic!("expected path callee, got {:?}", call.callee.kind);
    };
    assert_eq!(path.segments.len(), 2, "expected $package::bar");
    let first = &path.segments[0].ident;
    assert_eq!(first.as_str(&interner), "package");
    assert_eq!(
        first.origin,
        yelang_ast::tokenizer::IdentOrigin::Package,
        "first segment should have Package origin"
    );
    let second = &path.segments[1].ident;
    assert_eq!(second.as_str(&interner), "bar");
}
