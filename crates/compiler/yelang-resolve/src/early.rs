use yelang_ast::{Item, ItemKind, ModKind, Program};
use yelang_lexer::Span;

use crate::{
    imports::{UnresolvedImport, resolve_imports},
    module_tree::ModuleTree,
    scope::Resolver,
};

pub struct EarlyResolver<'a, 'b> {
    resolver: &'b mut Resolver<'a>,
}

impl<'a, 'b> EarlyResolver<'a, 'b> {
    pub fn new(resolver: &'b mut Resolver<'a>) -> Self {
        Self { resolver }
    }

    pub fn resolve(mut self, program: &Program) {
        self.collect_unresolved_imports(&program.items);
        resolve_imports(self.resolver);
    }

    fn collect_unresolved_imports(&mut self, items: &[Item]) {
        for item in items {
            self.collect_item(item);
        }
    }

    fn collect_item(&mut self, item: &Item) {
        match &item.kind {
            ItemKind::Module(m) => {
                if let ModKind::Inline { items } = &m.kind {
                    let old_module = self.resolver.current_module;
                    if let Some(id) = self.find_module_by_name(m.name.symbol) {
                        self.resolver.current_module = id;
                    }
                    self.collect_unresolved_imports(items);
                    self.resolver.current_module = old_module;
                }
            }
            ItemKind::Use(u) => {
                self.resolver.unresolved_imports.push(UnresolvedImport {
                    module_id: self.resolver.current_module,
                    tree: u.tree.clone(),
                    span: u.span,
                });
            }
            _ => {}
        }
    }

    fn find_module_by_name(&self, name: yelang_interner::Symbol) -> Option<yelang_util::DefId> {
        let current = self.resolver.current_module;
        self.resolver
            .module_tree
            .modules
            .get(&current)
            .and_then(|m| {
                m.items
                    .get(&crate::namespaces::Namespace::Type)
                    .and_then(|map| map.get(&name))
                    .copied()
            })
    }
}
