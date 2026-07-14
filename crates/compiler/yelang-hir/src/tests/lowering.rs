//! Lowering correctness tests.

use yelang_ast::Program;
use yelang_interner::Interner;
use yelang_lexer::{Span, TokenStream};
use yelang_util::{DefId, FxHashMap};

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

/// Build a minimal ResolvedCrate for tests that don't need full name resolution.
fn stub_resolved() -> ResolvedCrate {
    let root_id = DefId::new(1);
    let root_name = yelang_interner::Symbol::from(0u32);
    let root_node = yelang_resolve::ModuleNode::new(
        root_id,
        root_name,
        None,
        yelang_ast::Visibility::Public(Span::default()),
    );
    let mut modules = FxHashMap::default();
    modules.insert(root_id, root_node);
    let module_tree = yelang_resolve::ModuleTree::new(modules.get(&root_id).unwrap().clone());
    ResolvedCrate {
        module_tree,
        definitions: FxHashMap::default(),
        errors: vec![],
        def_resolutions: FxHashMap::default(),
    }
}

#[test]
fn lower_simple_fn() {
    let src = "fn main() { let x = 1; }";
    let (program, interner) = parse_program(src);
    let resolved = stub_resolved();
    let crate_hir = lower_crate(&program, &resolved, &interner);

    assert_eq!(crate_hir.items.len(), 1);
    let item = crate_hir.items.values().next().unwrap();
    assert!(matches!(item.kind, ItemKind::Fn { .. }));
}

#[test]
fn lower_struct_item() {
    let src = "struct Point { x: i32, y: i32 }";
    let (program, interner) = parse_program(src);
    let resolved = stub_resolved();
    let crate_hir = lower_crate(&program, &resolved, &interner);

    assert_eq!(crate_hir.items.len(), 1);
    let item = crate_hir.items.values().next().unwrap();
    assert!(matches!(item.kind, ItemKind::Struct { .. }));
}

#[test]
fn lower_enum_item() {
    let src = "enum Option { Some(i32), None }";
    let (program, interner) = parse_program(src);
    let resolved = stub_resolved();
    let crate_hir = lower_crate(&program, &resolved, &interner);

    assert_eq!(crate_hir.items.len(), 1);
    let item = crate_hir.items.values().next().unwrap();
    assert!(matches!(item.kind, ItemKind::Enum { .. }));
}

#[test]
fn lower_binary_expr() {
    let src = "fn add() { 1 + 2 }";
    let (program, interner) = parse_program(src);
    let resolved = stub_resolved();
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
    let resolved = stub_resolved();
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
    let resolved = stub_resolved();
    let crate_hir = lower_crate(&program, &resolved, &interner);

    let item = crate_hir.items.values().next().unwrap();
    let ItemKind::Fn { body, .. } = &item.kind else {
        panic!("expected fn");
    };
    let body = crate_hir.bodies.get(body).unwrap();
    assert!(matches!(body.value.kind, ExprKind::Block { .. }));
}
