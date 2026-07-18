//! Shared test utilities for HIR lowering tests.

use yelang_arena::{DefId, FxHashMap, IndexVec};
use yelang_ast::Program;
use yelang_interner::{Interner, Symbol};

use crate::res::ResolvedCrate;
use yelang_resolve::DefKind;
use yelang_resolve::lang_items::{LangItem, LangItems};

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
        definitions: IndexVec::default(),
        errors: vec![],
        def_resolutions: FxHashMap::default(),
        enum_variants: FxHashMap::default(),
        prelude: None,
        lang_items: LangItems::new(),
        generic_param_defs: FxHashMap::default(),
        generic_params: FxHashMap::default(),
    }
}

/// Build a minimal ResolvedCrate with the `Array` lang item registered.
/// Useful for tests that need `[T]` to lower to `Array<T>`.
pub fn stub_resolved_with_array() -> ResolvedCrate {
    let array_def_id = DefId::new(2);
    let mut resolved = stub_resolved();
    resolved.lang_items.insert(LangItem::Array, array_def_id);
    resolved
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

    let mut definitions = IndexVec::default();

    // Reserve DefId(1) for the synthetic root so that user definitions keep the
    // expected IDs starting at DefId(2).
    let root_def = yelang_resolve::def_collector::Definition {
        def_id: root_id,
        name: root_name,
        span: yelang_lexer::Span::default(),
        kind: yelang_resolve::DefKind::Module,
        parent: None,
        visibility: yelang_ast::Visibility::Public(yelang_lexer::Span::default()),
        lang_item: None,
    };
    let pushed_root = definitions.push(root_def);
    definitions[pushed_root].def_id = pushed_root;

    for (name, kind) in defs {
        let def_id = definitions.push(yelang_resolve::def_collector::Definition {
            def_id: DefId::new(1),
            name: *name,
            span: yelang_lexer::Span::default(),
            kind: *kind,
            parent: Some(root_id),
            visibility: yelang_ast::Visibility::Public(yelang_lexer::Span::default()),
            lang_item: None,
        });
        definitions[def_id].def_id = def_id;

        let ns = definitions[def_id]
            .namespace()
            .unwrap_or(yelang_resolve::Namespace::Type);
        root_node.add_item(ns, *name, def_id);
    }

    let mut modules = FxHashMap::default();
    modules.insert(root_id, root_node);
    let module_tree = yelang_resolve::ModuleTree::new(modules.get(&root_id).unwrap().clone());

    ResolvedCrate {
        module_tree,
        definitions,
        errors: vec![],
        def_resolutions: FxHashMap::default(),
        enum_variants: FxHashMap::default(),
        prelude: None,
        lang_items: LangItems::new(),
        generic_param_defs: FxHashMap::default(),
        generic_params: FxHashMap::default(),
    }
}
