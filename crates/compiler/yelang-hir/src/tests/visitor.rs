//! Tests for the HIR visitor.

use yelang_arena::DefId;
use yelang_ast::Program;
use yelang_interner::Interner;
use yelang_lexer::TokenStream;

use crate::crate_hir::Crate;
use crate::hir::{Expr, ExprKind, Item, Stmt};
use crate::hir_body::Body;
use crate::ids::BodyId;
use crate::lowering::lower_crate;
use crate::res::ResolvedCrate;
use crate::visitor::{Visitor, walk_crate};

struct ExprCounter<'hir> {
    count: usize,
    crate_hir: &'hir Crate,
}

impl<'hir> Visitor<'hir> for ExprCounter<'hir> {
    fn visit_expr(&mut self, _expr: &Expr) {
        self.count += 1;
    }

    fn visit_body_by_id(&mut self, body_id: BodyId) -> Option<&'hir Body> {
        self.crate_hir.bodies.get(&body_id)
    }
}

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
fn visitor_counts_expressions() {
    let src = "fn main() { 1 + 2 }";
    let (program, interner) = parse_program(src);
    let resolved = stub_resolved();
    let crate_hir = lower_crate(&program, &resolved, &interner);

    let mut counter = ExprCounter {
        count: 0,
        crate_hir: &crate_hir,
    };
    walk_crate(&mut counter, &crate_hir);
    assert!(counter.count > 0);
}
