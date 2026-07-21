//! Integration tests for `@intrinsic(...)` lowering and HIR shape.

use crate::hir::core::{ItemKind, Stmt};
use crate::hir::expr::Expr;
use crate::lowering::lower_crate;
use crate::res::ResolvedCrate;
use yelang_interner::Interner;

fn lower(src: &str) -> (crate::Crate, Interner, ResolvedCrate) {
    let interner = Interner::new();
    let mut stream = yelang_lexer::TokenKind::tokenize(src, &interner).expect("tokenize");
    let program = stream
        .parse::<yelang_ast::Program>()
        .expect("parse program");
    let resolved = yelang_resolve::resolve_crate(&program, &interner);
    let crate_hir = lower_crate(&program, &resolved, &interner);
    (crate_hir, interner, resolved)
}

#[test]
fn lower_intrinsic_expr_preserves_name_and_args() {
    let src = r#"
        fn main() {
            let plan = 0;
            let f = 0;
            let _ = @intrinsic("query_map", plan, f);
        }
    "#;
    let (crate_hir, interner, _resolved) = lower(src);

    let main_item = crate_hir
        .items
        .values()
        .filter_map(|opt| opt.as_ref())
        .find(|item| matches!(&item.kind, ItemKind::Fn { .. }))
        .expect("main function");
    let ItemKind::Fn { body, .. } = &main_item.kind else {
        panic!("expected fn item");
    };
    let body = crate_hir.body(*body).expect("main body");
    let block = crate_hir.expr(body.value).expect("body expr");
    let Expr::Block { block } = block else {
        panic!("expected block expression");
    };

    // The last statement is the let binding with the intrinsic call.
    let last_stmt = block.stmts.last().expect("at least one statement");
    let Stmt::Let { init, .. } = crate_hir.stmt(*last_stmt).expect("stmt") else {
        panic!("expected let statement");
    };
    let init = init.expect("let should have an initializer");
    let init_expr = crate_hir.expr(init).expect("init expr");
    let Expr::Intrinsic { name, args } = init_expr else {
        panic!("expected intrinsic expression, got {:?}", init_expr);
    };

    assert_eq!(
        interner.resolve(&name.symbol),
        "intrinsic",
        "intrinsic namespace should be 'intrinsic'"
    );
    assert_eq!(args.len(), 3, "expected three arguments: name, plan, f");
}

#[test]
fn lower_empty_intrinsic_has_no_args() {
    let src = r#"
        fn main() {
            let _ = @noop();
        }
    "#;
    let (crate_hir, interner, _resolved) = lower(src);

    let main_item = crate_hir
        .items
        .values()
        .filter_map(|opt| opt.as_ref())
        .find(|item| matches!(&item.kind, ItemKind::Fn { .. }))
        .expect("main function");
    let ItemKind::Fn { body, .. } = &main_item.kind else {
        panic!("expected fn item");
    };
    let body = crate_hir.body(*body).expect("main body");
    let block = crate_hir.expr(body.value).expect("body expr");
    let Expr::Block { block } = block else {
        panic!("expected block expression");
    };
    let last_stmt = block.stmts.last().expect("at least one statement");
    let Stmt::Let { init, .. } = crate_hir.stmt(*last_stmt).expect("stmt") else {
        panic!("expected let statement");
    };
    let init = init.expect("let should have an initializer");
    let init_expr = crate_hir.expr(init).expect("init expr");
    let Expr::Intrinsic { name, args } = init_expr else {
        panic!("expected intrinsic expression");
    };

    assert_eq!(name.symbol, interner.get_or_intern("noop"));
    assert!(args.is_empty(), "expected no arguments");
}
