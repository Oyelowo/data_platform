use yelang_interner::Symbol;
use yelang_lexer::Span;
use yelang_util::{DefId, FxHashMap};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefKind {
    Module,
    Struct,
    Enum,
    EnumVariant,
    TypeAlias,
    Trait,
    Fn,
    Const,
    Static,
    Impl,
    Use,
    Field,
    Local,
    Param,
}

#[derive(Debug, Clone)]
pub struct Definition {
    pub def_id: DefId,
    pub name: Symbol,
    pub span: Span,
    pub kind: DefKind,
    pub parent: Option<DefId>,
}

impl Definition {
    pub fn namespace(&self) -> Option<crate::namespaces::Namespace> {
        use crate::namespaces::Namespace;
        match self.kind {
            DefKind::Module
            | DefKind::Struct
            | DefKind::Enum
            | DefKind::EnumVariant
            | DefKind::TypeAlias
            | DefKind::Trait => Some(Namespace::Type),
            DefKind::Fn | DefKind::Const | DefKind::Static | DefKind::Local | DefKind::Param => {
                Some(Namespace::Value)
            }
            DefKind::Impl | DefKind::Use | DefKind::Field => None,
        }
    }
}

use crate::{
    error::ResolutionError,
    module_tree::{ModuleNode, ModuleTree},
};
use yelang_ast::{FnDef, Ident, Item, ItemKind, ModDef, ModKind};
use yelang_ast::item::{Const, Enum, Impl, Static, Struct, Trait, TypeAlias, Use};
use yelang_interner::Interner;

pub struct DefCollector<'a> {
    interner: &'a Interner,
    next_def_id: u32,
    pub definitions: FxHashMap<DefId, Definition>,
    pub module_tree: ModuleTree,
    pub current_module: DefId,
    pub errors: Vec<ResolutionError>,
}

impl<'a> DefCollector<'a> {
    pub fn new(interner: &'a Interner) -> Self {
        let root_id = DefId::new(1);
        let root_name = interner.get_or_intern("crate");
        let root_node = ModuleNode::new(root_id, root_name, None);
        let mut definitions = FxHashMap::new();
        definitions.insert(
            root_id,
            Definition {
                def_id: root_id,
                name: root_name,
                span: Span::default(),
                kind: DefKind::Module,
                parent: None,
            },
        );
        Self {
            interner,
            next_def_id: 2,
            definitions,
            module_tree: ModuleTree::new(root_node),
            current_module: root_id,
            errors: Vec::new(),
        }
    }

    pub fn collect(mut self, program: &yelang_ast::Program) -> Self {
        self.collect_items(&program.items);
        self
    }

    fn next_def_id(&mut self) -> DefId {
        let id = DefId::new(self.next_def_id);
        self.next_def_id += 1;
        id
    }

    fn collect_items(&mut self, items: &[Item]) {
        for item in items {
            self.collect_item(item);
        }
    }

    fn collect_item(&mut self, item: &Item) {
        match &item.kind {
            ItemKind::Fn(func) => self.collect_fn(func, item.span),
            ItemKind::Struct(s) => self.collect_struct(s, item.span),
            ItemKind::Enum(e) => self.collect_enum(e, item.span),
            ItemKind::TypeAlias(ta) => self.collect_type_alias(ta, item.span),
            ItemKind::Trait(t) => self.collect_trait(t, item.span),
            ItemKind::Module(m) => self.collect_module(m, item.span),
            ItemKind::Const(c) => self.collect_const(c, item.span),
            ItemKind::Static(s) => self.collect_static(s, item.span),
            ItemKind::Impl(i) => self.collect_impl(i, item.span),
            ItemKind::Use(u) => self.collect_use(u, item.span),
        }
    }

    fn collect_fn(&mut self, func: &FnDef, span: Span) {
        let def_id = self.next_def_id();
        let name = func.name.symbol;
        self.add_def(def_id, name, span, DefKind::Fn);
        self.add_to_module(crate::namespaces::Namespace::Value, name, def_id, span);
    }

    fn collect_struct(&mut self, s: &Struct, span: Span) {
        let def_id = self.next_def_id();
        let name = s.name.symbol;
        self.add_def(def_id, name, span, DefKind::Struct);
        self.add_to_module(crate::namespaces::Namespace::Type, name, def_id, span);
    }

    fn collect_enum(&mut self, e: &Enum, span: Span) {
        let def_id = self.next_def_id();
        let name = e.name.symbol;
        self.add_def(def_id, name, span, DefKind::Enum);
        self.add_to_module(crate::namespaces::Namespace::Type, name, def_id, span);
        // Variants are also definitions in the type namespace.
        for variant in &e.variants {
            let vdef_id = self.next_def_id();
            let vname = variant.name.symbol;
            self.add_def(vdef_id, vname, variant.span, DefKind::EnumVariant);
            // Variants are accessible in the type namespace of the current module.
            self.add_to_module(crate::namespaces::Namespace::Type, vname, vdef_id, variant.span);
        }
    }

    fn collect_type_alias(&mut self, ta: &TypeAlias, span: Span) {
        let def_id = self.next_def_id();
        let name = ta.name.symbol;
        self.add_def(def_id, name, span, DefKind::TypeAlias);
        self.add_to_module(crate::namespaces::Namespace::Type, name, def_id, span);
    }

    fn collect_trait(&mut self, t: &Trait, span: Span) {
        let def_id = self.next_def_id();
        let name = t.name.symbol;
        self.add_def(def_id, name, span, DefKind::Trait);
        self.add_to_module(crate::namespaces::Namespace::Type, name, def_id, span);
    }

    fn collect_module(&mut self, m: &ModDef, span: Span) {
        let def_id = self.next_def_id();
        let name = m.name.symbol;
        let parent = self.current_module;
        self.add_def(def_id, name, span, DefKind::Module);
        self.add_to_module(crate::namespaces::Namespace::Type, name, def_id, span);

        let mut node = ModuleNode::new(def_id, name, Some(parent));
        let old_module = self.current_module;
        self.current_module = def_id;

        // Pre-populate node into the tree so nested items can reference it.
        self.module_tree.modules.insert(def_id, node.clone());

        if let ModKind::Inline { items } = &m.kind {
            self.collect_items(items);
            node = self.module_tree.modules.get(&def_id).cloned().unwrap_or(node);
        }

        self.current_module = old_module;
        self.module_tree.modules.insert(def_id, node);

        if let Some(parent_node) = self.module_tree.modules.get_mut(&parent) {
            parent_node.children.push(def_id);
        }
    }

    fn collect_const(&mut self, c: &Const, span: Span) {
        let def_id = self.next_def_id();
        let name = c.name.symbol;
        self.add_def(def_id, name, span, DefKind::Const);
        self.add_to_module(crate::namespaces::Namespace::Value, name, def_id, span);
    }

    fn collect_static(&mut self, s: &Static, span: Span) {
        let def_id = self.next_def_id();
        let name = s.name.symbol;
        self.add_def(def_id, name, span, DefKind::Static);
        self.add_to_module(crate::namespaces::Namespace::Value, name, def_id, span);
    }

    fn collect_impl(&mut self, _i: &Impl, span: Span) {
        let def_id = self.next_def_id();
        // Impl blocks don't have a single name; use a synthetic name.
        let name = self.interner.get_or_intern("<impl>");
        self.add_def(def_id, name, span, DefKind::Impl);
        // Impl items are not added to the module namespace directly.
    }

    fn collect_use(&mut self, _u: &Use, span: Span) {
        let def_id = self.next_def_id();
        let name = self.interner.get_or_intern("<use>");
        self.add_def(def_id, name, span, DefKind::Use);
        // Use items are resolved later during early resolution.
    }

    fn add_def(&mut self, def_id: DefId, name: Symbol, span: Span, kind: DefKind) {
        self.definitions.insert(
            def_id,
            Definition {
                def_id,
                name,
                span,
                kind,
                parent: Some(self.current_module),
            },
        );
    }

    fn add_to_module(
        &mut self,
        ns: crate::namespaces::Namespace,
        name: Symbol,
        def_id: DefId,
        span: Span,
    ) {
        if let Some(module) = self.module_tree.modules.get_mut(&self.current_module) {
            if let Some(existing) = module.add_item(ns, name, def_id) {
                let existing_span = self
                    .definitions
                    .get(&existing)
                    .map(|d| d.span)
                    .unwrap_or_else(Span::default);
                self.errors.push(ResolutionError::DuplicateDefinition {
                    name,
                    span,
                    original_span: existing_span,
                });
            }
        }
    }
}
