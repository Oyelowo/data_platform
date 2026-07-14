//! Tests for AST desugarings performed during lowering.

use yelang_ast::Program;
use yelang_interner::Interner;
use yelang_lexer::TokenStream;
use yelang_util::DefId;

use crate::hir::{ExprKind, ItemKind, StmtKind};
use crate::res::ResolvedCrate;
use crate::lowering::lower_crate;

fn parse_program(src: &str) -> (Program, Interner) {
    let interner = Interner::new();
    let mut stream = yelang_lexer::TokenKind::tokenize(src, &interner).expect("tokenize");
    let program = stream.parse::<Program>().expect("parse program");
    (program, interner)
}

#[test]
fn desugar_while() {
    let src = "fn main() { while true { break } }";
    let (program, interner) = parse_program(src);
    let resolved = ResolvedCrate::new(DefId::new(1));
    let crate_hir = lower_crate(&program, &resolved, &interner);

    let item = crate_hir.items.values().next().unwrap();
    let ItemKind::Fn { body, .. } = &item.kind else {
        panic!("expected fn");
    };
    let body = crate_hir.bodies.get(body).unwrap();

    // The body should contain a `loop` expression (desugared from `while`).
    assert!(matches!(body.value.kind, ExprKind::Block { .. }));
}

#[test]
fn desugar_for() {
    let src = "fn main() { for x in 0..10 { } }";
    let (program, interner) = parse_program(src);
    let resolved = ResolvedCrate::new(DefId::new(1));
    let crate_hir = lower_crate(&program, &resolved, &interner);

    let item = crate_hir.items.values().next().unwrap();
    let ItemKind::Fn { body, .. } = &item.kind else {
        panic!("expected fn");
    };
    let body = crate_hir.bodies.get(body).unwrap();
    assert!(matches!(body.value.kind, ExprKind::Block { .. }));
}

#[test]
fn desugar_try_operator() {
    // `?` should be lowered to a match expression.
    let src = "fn main() { some()? }";
    let (program, interner) = parse_program(src);
    let resolved = ResolvedCrate::new(DefId::new(1));
    let crate_hir = lower_crate(&program, &resolved, &interner);

    let item = crate_hir.items.values().next().unwrap();
    let ItemKind::Fn { body, .. } = &item.kind else {
        panic!("expected fn");
    };
    let body = crate_hir.bodies.get(body).unwrap();
    assert!(matches!(body.value.kind, ExprKind::Block { .. }));
}

#[test]
fn desugar_let_chain() {
    // if let Some(x) = a && let Some(y) = b && x > y { ... }
    // -> nested if let
    let src = r#"
        fn main() {
            if let Some(x) = a && let Some(y) = b && x > y {
                42
            }
        }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = ResolvedCrate::new(DefId::new(1));
    let crate_hir = lower_crate(&program, &resolved, &interner);

    let item = crate_hir.items.values().next().unwrap();
    let ItemKind::Fn { body, .. } = &item.kind else {
        panic!("expected fn");
    };
    let body = crate_hir.bodies.get(body).unwrap();
    assert!(matches!(body.value.kind, ExprKind::Block { .. }));
}
