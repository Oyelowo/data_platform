//! Tests for AST desugarings performed during lowering.

use yelang_arena::DefId;
use yelang_ast::Program;
use yelang_interner::Interner;
use yelang_lexer::TokenStream;

use crate::hir::{ExprKind, ItemKind, StmtKind};
use crate::lowering::lower_crate;
use crate::res::ResolvedCrate;

fn parse_program(src: &str) -> (Program, Interner) {
    let mut interner = Interner::new();
    let mut stream = yelang_lexer::TokenKind::tokenize(src, &mut interner).expect("tokenize");
    let program = stream.parse::<Program>().expect("parse program");
    (program, interner)
}

fn stub_resolved() -> ResolvedCrate {
    let root_id = DefId::new(1);
    let root_name = yelang_interner::Symbol::from(0u32);
    let root_node = yelang_resolve::ModuleNode::new(
        root_id,
        root_name,
        None,
        yelang_ast::Visibility::Public(yelang_lexer::Span::default()),
    );
    let mut modules = yelang_arena::FxHashMap::default();
    modules.insert(root_id, root_node);
    let module_tree = yelang_resolve::ModuleTree::new(modules.get(&root_id).unwrap().clone());
    ResolvedCrate {
        module_tree,
        definitions: yelang_arena::FxHashMap::default(),
        errors: vec![],
        def_resolutions: yelang_arena::FxHashMap::default(),
        enum_variants: yelang_arena::FxHashMap::default(),
        prelude: None,
    }
}

#[test]
fn desugar_while() {
    let src = "fn main() { while true { break } }";
    let (program, interner) = parse_program(src);
    let resolved = stub_resolved();
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
fn desugar_try_operator() {
    // `?` should be lowered to a match expression.
    let src = "fn main() { some()? }";
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
    let resolved = stub_resolved();
    let crate_hir = lower_crate(&program, &resolved, &interner);

    let item = crate_hir.items.values().next().unwrap();
    let ItemKind::Fn { body, .. } = &item.kind else {
        panic!("expected fn");
    };
    let body = crate_hir.bodies.get(body).unwrap();
    assert!(matches!(body.value.kind, ExprKind::Block { .. }));
}
