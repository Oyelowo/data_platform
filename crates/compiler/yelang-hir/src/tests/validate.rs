//! Tests for the HIR validation pass.

use yelang_arena::{DefId, FxHashMap};
use yelang_ast::Program;
use yelang_interner::Interner;
use yelang_lexer::Span;

use crate::lowering::lower_crate;
use crate::res::ResolvedCrate;
use crate::validate::validate_hir;

fn parse_program(src: &str) -> (Program, Interner) {
    let interner = Interner::new();
    let mut stream = yelang_lexer::TokenKind::tokenize(src, &interner).expect("tokenize");
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
        yelang_ast::Visibility::Public(Span::default()),
    );
    let mut modules = FxHashMap::default();
    modules.insert(root_id, root_node);
    let module_tree = yelang_resolve::ModuleTree::new(modules.get(&root_id).unwrap().clone());
    ResolvedCrate {
        module_tree,
        definitions: yelang_arena::IndexVec::default(),
        errors: vec![],
        def_resolutions: FxHashMap::default(),
        enum_variants: FxHashMap::default(),
        prelude: None,
        generic_param_defs: FxHashMap::default(),
        generic_params: FxHashMap::default(),
    }
}

#[test]
fn validate_simple_fn_has_no_errors() {
    let src = "fn main() { let x = 1; }";
    let (program, interner) = parse_program(src);
    let resolved = stub_resolved();
    let crate_hir = lower_crate(&program, &resolved, &interner);

    let errors = validate_hir(&crate_hir);
    assert!(errors.is_empty(), "expected no validation errors, got: {:?}", errors);
}

#[test]
fn validate_catches_unallocated_expr_id() {
    let src = "fn main() { let x = 1; }";
    let (program, interner) = parse_program(src);
    let resolved = stub_resolved();
    let mut crate_hir = lower_crate(&program, &resolved, &interner);

    // Replace the body value with a synthetic, unallocated ExprId.
    let body_id = crate_hir.bodies.iter().next().map(|(id, _)| id).unwrap();
    crate_hir.bodies.get_mut(body_id).unwrap().value = crate::ids::ExprId::default();

    let errors = validate_hir(&crate_hir);
    assert!(!errors.is_empty(), "expected a validation error for unallocated ExprId");
    assert!(
        errors.iter().any(|e| e.message.contains("ExprId is not allocated")),
        "expected ExprId allocation error, got: {:?}",
        errors
    );
}
