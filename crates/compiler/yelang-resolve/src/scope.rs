use yelang_arena::{DefId, FxHashMap, IndexVec};
use yelang_interner::{Interner, Symbol};
use yelang_lexer::Span;

use crate::{
    def_collector::Definition,
    error::ResolutionError,
    lang_items::LangItems,
    module_tree::ModuleTree,
    namespaces::Namespace,
    prelude::Prelude,
    rib::{Resolution, Rib},
};

pub struct Resolver<'a> {
    pub interner: &'a Interner,
    pub module_tree: ModuleTree,
    pub next_local_id: u32,
    pub value_ribs: Vec<Rib>,
    pub type_ribs: Vec<Rib>,
    pub unresolved_imports: Vec<crate::imports::UnresolvedImport>,
    pub errors: Vec<ResolutionError>,
    pub definitions: IndexVec<DefId, Definition>,
    pub current_module: DefId,
    /// Maps type name ( Symbol) to the DefId of impl blocks for that type
    pub inherent_impls: FxHashMap<Symbol, Vec<DefId>>,
    /// Maps (trait_name, type_name) to DefId of trait impl blocks
    pub trait_impls: FxHashMap<(Symbol, Symbol), Vec<DefId>>,
    /// Maps impl DefId to the names of its items (for fast lookup)
    pub impl_item_names: FxHashMap<DefId, FxHashMap<Symbol, DefId>>,
    /// The actual type name being implemented when inside an `impl` block.
    /// Used to resolve `Self::item` paths.
    pub self_type: Option<Symbol>,
    /// Standard prelude items. Checked as a final fallback so they can be
    /// shadowed by any local definition or import.
    pub prelude: Option<Prelude>,
    /// Registry of language items discovered during def collection.
    pub lang_items: LangItems,
    /// Maps enum DefId to a map of (variant_name -> variant DefId).
    pub enum_variants: FxHashMap<DefId, FxHashMap<Symbol, DefId>>,
    /// Maps path spans to resolved DefIds for non-local paths.
    /// Populated during late resolution and consumed by HIR lowering.
    pub def_resolutions: FxHashMap<Span, DefId>,
}

impl<'a> Resolver<'a> {
    pub fn new(
        interner: &'a Interner,
        module_tree: ModuleTree,
        definitions: IndexVec<DefId, Definition>,
        prelude: Option<Prelude>,
        lang_items: LangItems,
        enum_variants: FxHashMap<DefId, FxHashMap<Symbol, DefId>>,
    ) -> Self {
        Self {
            interner,
            module_tree,
            next_local_id: 1,
            value_ribs: Vec::new(),
            type_ribs: Vec::new(),
            unresolved_imports: Vec::new(),
            errors: Vec::new(),
            definitions,
            current_module: DefId::new(1),
            inherent_impls: FxHashMap::default(),
            trait_impls: FxHashMap::default(),
            impl_item_names: FxHashMap::default(),
            self_type: None,
            prelude,
            lang_items,
            enum_variants,
            def_resolutions: FxHashMap::default(),
        }
    }

    pub fn push_rib(&mut self, kind: crate::rib::RibKind) {
        self.value_ribs.push(Rib::new(kind));
        self.type_ribs.push(Rib::new(kind));
    }

    pub fn pop_rib(&mut self) {
        self.value_ribs.pop();
        self.type_ribs.pop();
    }

    pub fn resolve_name(&self, ns: Namespace, name: Symbol, use_span: Span) -> Option<Resolution> {
        let ribs = match ns {
            Namespace::Value => &self.value_ribs,
            Namespace::Type => &self.type_ribs,
        };
        for rib in ribs.iter().rev() {
            if let Some((res, _)) = rib.get_with_span(ns, name) {
                return Some(res);
            }
        }
        // Look up in the current module and its ancestors.
        let mut module_id = self.current_module;
        while let Some(module) = self.module_tree.modules.get(&module_id) {
            // Check primary namespace first.
            if let Some(def_id) = module.get_item(ns, name)
                && let Some(res) = self.resolve_visible_module_item(def_id, use_span)
            {
                return Some(res);
            }
            // For value namespace, also check type namespace (modules are types).
            if ns == Namespace::Value
                && let Some(def_id) = module.get_item(Namespace::Type, name)
                && let Some(res) = self.resolve_visible_module_item(def_id, use_span)
            {
                return Some(res);
            }
            if let Some(parent) = module.parent {
                module_id = parent;
            } else {
                break;
            }
        }
        // Final fallback: check the prelude. Prelude items are shadowable by
        // any rib or module item, matching Rust semantics (RFC 1560).
        if let Some(prelude) = &self.prelude
            && let Some(def_id) = prelude.items.get(&ns).and_then(|m| m.get(&name)).copied()
        {
            return Some(Resolution::Def { def_id });
        }
        None
    }

    fn resolve_visible_module_item(&self, def_id: DefId, _use_span: Span) -> Option<Resolution> {
        Some(Resolution::Def { def_id })
    }

    pub fn resolve_name_in_module(
        &self,
        module_id: DefId,
        ns: Namespace,
        name: Symbol,
        _use_span: Span,
    ) -> Option<DefId> {
        self.module_tree
            .modules
            .get(&module_id)
            .and_then(|m| m.get_item(ns, name))
    }

    pub fn next_local_id(&mut self) -> u32 {
        let id = self.next_local_id;
        self.next_local_id += 1;
        id
    }

    pub fn record_error(&mut self, err: ResolutionError) {
        self.errors.push(err);
    }
}
