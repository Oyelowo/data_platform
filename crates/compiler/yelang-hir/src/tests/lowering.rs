//! Lowering correctness tests.

use yelang_ast::Program;
use yelang_interner::Interner;
use yelang_lexer::{Span, TokenStream};
use yelang_util::DefId;

use crate::hir::{ExprKind, ItemKind, StmtKind};
use crate::map::Map;
use crate::res::ResolvedCrate;
use crate::lowering::lower_crate;
use crate::crate_hir::Crate;

fn parse_program(src: &str) -> (Program, Interner) {
    let interner = Interner::new();
    let mut stream = yelang_lexer::TokenKind::tokenize(src, &interner).expect("tokenize");
    let program = stream.parse::<Program>().expect("parse program");
    (program, interner)
}

#[test]
fn lower_simple_fn() {
    let src = "fn main() { let x = 1; }";
    let (program, interner) = parse_program(src);
    let resolved = ResolvedCrate::new(DefId::new(1));
    let crate_hir = lower_crate(&program, &resolved, &interner);

    assert_eq!(crate_hir.items.len(), 1);
    let item = crate_hir.items.values().next().unwrap();
    assert!(matches!(item.kind, ItemKind::Fn { .. }));
}

#[test]
fn lower_struct_item() {
    let src = "struct Point { x: i32, y: i32 }";
    let (program, interner) = parse_program(src);
    let resolved = ResolvedCrate::new(DefId::new(1));
    let crate_hir = lower_crate(&program, &resolved, &interner);

    assert_eq!(crate_hir.items.len(), 1);
    let item = crate_hir.items.values().next().unwrap();
    assert!(matches!(item.kind, ItemKind::Struct { .. }));
}

#[test]
fn lower_enum_item() {
    let src = "enum Option { Some(i32), None }";
    let (program, interner) = parse_program(src);
    let resolved = ResolvedCrate::new(DefId::new(1));
    let crate_hir = lower_crate(&program, &resolved, &interner);

    assert_eq!(crate_hir.items.len(), 1);
    let item = crate_hir.items.values().next().unwrap();
    assert!(matches!(item.kind, ItemKind::Enum { .. }));
}

#[test]
fn lower_binary_expr() {
    let src = "fn add() { 1 + 2 }";
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
fn lower_call_expr() {
    let src = "fn foo() { bar(1, 2) }";
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
fn lower_match_expr() {
    let src = r#"
        fn test() {
            match 1 {
                1 => 2,
                _ => 3,
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
