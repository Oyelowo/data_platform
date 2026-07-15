use yelang_arena::{DefId, FxHashMap};
use yelang_ast::Visibility;
use yelang_interner::Symbol;

use crate::namespaces::Namespace;

#[derive(Debug, Clone)]
pub struct ModuleTree {
    pub root: ModuleNode,
    pub modules: FxHashMap<DefId, ModuleNode>,
}

impl ModuleTree {
    pub fn new(root: ModuleNode) -> Self {
        let mut modules = FxHashMap::new();
        modules.insert(root.def_id, root.clone());
        Self { root, modules }
    }
}

#[derive(Debug, Clone)]
pub struct ModuleNode {
    pub def_id: DefId,
    pub name: Symbol,
    pub parent: Option<DefId>,
    pub children: Vec<DefId>,
    pub items: FxHashMap<Namespace, FxHashMap<Symbol, DefId>>,
    pub visibility: Visibility,
}

impl ModuleNode {
    pub fn new(def_id: DefId, name: Symbol, parent: Option<DefId>, visibility: Visibility) -> Self {
        Self {
            def_id,
            name,
            parent,
            children: Vec::new(),
            items: FxHashMap::new(),
            visibility,
        }
    }

    pub fn add_item(&mut self, ns: Namespace, name: Symbol, def_id: DefId) -> Option<DefId> {
        let ns_map = self.items.entry(ns).or_insert_with(FxHashMap::new);
        ns_map.insert(name, def_id)
    }

    pub fn get_item(&self, ns: Namespace, name: Symbol) -> Option<DefId> {
        self.items.get(&ns).and_then(|m| m.get(&name)).copied()
    }
}
