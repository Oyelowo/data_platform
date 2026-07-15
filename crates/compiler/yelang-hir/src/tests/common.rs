//! Shared test utilities for HIR lowering tests.

use yelang_ast::Program;
use yelang_interner::{Interner, Symbol};
use yelang_lexer::TokenStream;
use yelang_util::{DefId, FxHashMap};

use crate::res::ResolvedCrate;
use yelang_resolve::DefKind;

pub fn parse_program(src: &str) -> (Program, Interner) {
    let interner = Interner::new();
    let mut stream = yelang_lexer::TokenKind::tokenize(src, &interner).expect("tokenize");
    let program = stream.parse::<Program>().expect("parse program");
    (program, interner)
}

/// Build a minimal ResolvedCrate for tests that don't need full name resolution.
pub fn stub_resolved() -> ResolvedCrate {
    let root_id = DefId::new(1);
    let root_name = yelang_interner::Symbol::from(0u32);
    let root_node = yelang_resolve::ModuleNode::new(
        root_id,
        root_name,
        None,
        yelang_ast::Visibility::Public(yelang_lexer::Span::default()),
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

/// Build a ResolvedCrate with the given definitions pre-populated.
/// Useful for tests that need name resolution to succeed for specific items.
pub fn resolved_with_defs(defs: &[(Symbol, DefKind)]) -> ResolvedCrate {
    let root_id = DefId::new(1);
    let root_name = yelang_interner::Symbol::from(0u32);
    let mut root_node = yelang_resolve::ModuleNode::new(
        root_id,
        root_name,
        None,
        yelang_ast::Visibility::Public(yelang_lexer::Span::default()),
    );

    let mut definitions = FxHashMap::default();
    let mut def_id = DefId::new(2);

    for (name, kind) in defs {
        let definition = yelang_resolve::def_collector::Definition {
            def_id,
            name: *name,
            span: yelang_lexer::Span::default(),
            kind: *kind,
            parent: Some(root_id),
            visibility: yelang_ast::Visibility::Public(yelang_lexer::Span::default()),
            lang_item: None,
        };
        let ns = definition
            .namespace()
            .unwrap_or(yelang_resolve::Namespace::Type);
        definitions.insert(def_id, definition);
        root_node.add_item(ns, *name, def_id);
        def_id = DefId::new(def_id.raw() + 1);
    }

    let mut modules = FxHashMap::default();
    modules.insert(root_id, root_node);
    let module_tree = yelang_resolve::ModuleTree::new(modules.get(&root_id).unwrap().clone());

    ResolvedCrate {
        module_tree,
        definitions,
        errors: vec![],
        def_resolutions: FxHashMap::default(),
    }
}
