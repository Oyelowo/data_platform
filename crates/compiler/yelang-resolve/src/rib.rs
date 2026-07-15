use yelang_arena::{DefId, FxHashMap};
use yelang_interner::Symbol;

use crate::namespaces::Namespace;

#[derive(Debug, Clone)]
pub struct Rib {
    pub kind: RibKind,
    pub bindings: FxHashMap<Namespace, FxHashMap<Symbol, Resolution>>,
}

impl Rib {
    pub fn new(kind: RibKind) -> Self {
        Self {
            kind,
            bindings: FxHashMap::new(),
        }
    }

    pub fn insert(&mut self, ns: Namespace, name: Symbol, res: Resolution) {
        let ns_map = self.bindings.entry(ns).or_insert_with(FxHashMap::new);
        ns_map.insert(name, res);
    }

    pub fn get(&self, ns: Namespace, name: Symbol) -> Option<Resolution> {
        self.bindings.get(&ns).and_then(|m| m.get(&name)).copied()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RibKind {
    Module,
    Fn,
    Block,
    Loop,
    Pat,
    Opaque,
    Macro,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    Def { def_id: DefId },
    Local { local_id: u32 },
    Import { import_id: DefId },
    Err,
}
