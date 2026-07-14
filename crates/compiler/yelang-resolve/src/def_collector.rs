use yelang_ast::Visibility;
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
    pub visibility: Visibility,
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
use yelang_ast::{FnDef, Ident, Item, ItemKind, ModDef, ModKind, Type, TypeKind};
use yelang_ast::item::{Const, Enum, Impl, ImplItemKind, Static, Struct, Trait, TypeAlias, Use};
use yelang_interner::Interner;

pub struct DefCollector<'a> {
    interner: &'a Interner,
    next_def_id: u32,
    pub definitions: FxHashMap<DefId, Definition>,
    pub module_tree: ModuleTree,
    pub current_module: DefId,
    pub errors: Vec<ResolutionError>,
    /// Maps type name (as Symbol) to the DefId of impl blocks for that type
    pub inherent_impls: FxHashMap<Symbol, Vec<DefId>>,
    /// Maps (trait_name, type_name) to DefId of trait impl blocks
    pub trait_impls: FxHashMap<(Symbol, Symbol), Vec<DefId>>,
    /// Maps impl DefId to the names of its items (for fast lookup)
    pub impl_item_names: FxHashMap<DefId, FxHashMap<Symbol, DefId>>,
}

impl<'a> DefCollector<'a> {
    pub fn new(interner: &'a Interner) -> Self {
        let root_id = DefId::new(1);
        let root_name = interner.get_or_intern("crate");
        let root_node = ModuleNode::new(root_id, root_name, None, Visibility::Public(Span::default()));
        let mut definitions = FxHashMap::new();
        definitions.insert(
            root_id,
            Definition {
                def_id: root_id,
                name: root_name,
                span: Span::default(),
                kind: DefKind::Module,
                parent: None,
                visibility: Visibility::Public(Span::default()),
            },
        );
        let mut collector = Self {
            interner,
            next_def_id: 2,
            definitions,
            module_tree: ModuleTree::new(root_node),
            current_module: root_id,
            errors: Vec::new(),
            inherent_impls: FxHashMap::default(),
            trait_impls: FxHashMap::default(),
            impl_item_names: FxHashMap::default(),
        };
        collector.seed_primitives();
        collector
    }

    fn seed_primitives(&mut self) {
        let primitives = [
            "i8", "i16", "i32", "i64", "i128", "isize",
            "u8", "u16", "u32", "u64", "u128", "usize",
            "f32", "f64", "bool", "char", "str",
        ];
        for name in primitives {
            let symbol = self.interner.get_or_intern(name);
            let def_id = self.next_def_id();
            self.add_def(def_id, symbol, Span::default(), DefKind::TypeAlias, Visibility::Public(Span::default()));
            self.add_to_module(crate::namespaces::Namespace::Type, symbol, def_id, Span::default());
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
            ItemKind::Fn(func) => self.collect_fn(func, item.span, item.visibility.clone()),
            ItemKind::Struct(s) => self.collect_struct(s, item.span, item.visibility.clone()),
            ItemKind::Enum(e) => self.collect_enum(e, item.span, item.visibility.clone()),
            ItemKind::TypeAlias(ta) => self.collect_type_alias(ta, item.span, item.visibility.clone()),
            ItemKind::Trait(t) => self.collect_trait(t, item.span, item.visibility.clone()),
            ItemKind::Module(m) => self.collect_module(m, item.span, item.visibility.clone()),
            ItemKind::Const(c) => self.collect_const(c, item.span, item.visibility.clone()),
            ItemKind::Static(s) => self.collect_static(s, item.span, item.visibility.clone()),
            ItemKind::Impl(i) => self.collect_impl(i, item.span, item.visibility.clone()),
            ItemKind::Use(u) => self.collect_use(u, item.span, item.visibility.clone()),
        }
    }

    fn collect_fn(&mut self, func: &FnDef, span: Span, visibility: Visibility) {
        let def_id = self.next_def_id();
        let name = func.name.symbol;
        self.add_def(def_id, name, span, DefKind::Fn, visibility);
        self.add_to_module(crate::namespaces::Namespace::Value, name, def_id, span);
    }

    fn collect_struct(&mut self, s: &Struct, span: Span, visibility: Visibility) {
        let def_id = self.next_def_id();
        let name = s.name.symbol;
        self.add_def(def_id, name, span, DefKind::Struct, visibility.clone());
        self.add_to_module(crate::namespaces::Namespace::Type, name, def_id, span);

        // Collect struct fields as definitions with their own visibility
        if let yelang_ast::StructFields::Named(fields) = &s.fields {
            for field in fields {
                let fdef_id = self.next_def_id();
                let fname = field.name.symbol;
                self.add_def(fdef_id, fname, field.span, DefKind::Field, field.visibility.clone());
                // Fields are children of the struct, not directly in module namespace
            }
        }
    }

    fn collect_enum(&mut self, e: &Enum, span: Span, visibility: Visibility) {
        let def_id = self.next_def_id();
        let name = e.name.symbol;
        self.add_def(def_id, name, span, DefKind::Enum, visibility.clone());
        self.add_to_module(crate::namespaces::Namespace::Type, name, def_id, span);
        // Variants are definitions in both the type and value namespaces.
        // Inherit visibility from the parent enum.
        for variant in &e.variants {
            let vdef_id = self.next_def_id();
            let vname = variant.name.symbol;
            let variant_vis = if visibility.is_public() {
                Visibility::Public(variant.span)
            } else {
                visibility.clone()
            };
            self.add_def(vdef_id, vname, variant.span, DefKind::EnumVariant, variant_vis);
            self.add_to_module(crate::namespaces::Namespace::Type, vname, vdef_id, variant.span);
            self.add_to_module(crate::namespaces::Namespace::Value, vname, vdef_id, variant.span);
        }
    }

    fn collect_type_alias(&mut self, ta: &TypeAlias, span: Span, visibility: Visibility) {
        let def_id = self.next_def_id();
        let name = ta.name.symbol;
        self.add_def(def_id, name, span, DefKind::TypeAlias, visibility);
        self.add_to_module(crate::namespaces::Namespace::Type, name, def_id, span);
    }

    fn collect_trait(&mut self, t: &Trait, span: Span, visibility: Visibility) {
        let def_id = self.next_def_id();
        let name = t.name.symbol;
        self.add_def(def_id, name, span, DefKind::Trait, visibility);
        self.add_to_module(crate::namespaces::Namespace::Type, name, def_id, span);
    }

    fn collect_module(&mut self, m: &ModDef, span: Span, visibility: Visibility) {
        let def_id = self.next_def_id();
        let name = m.name.symbol;
        let parent = self.current_module;
        self.add_def(def_id, name, span, DefKind::Module, visibility.clone());
        self.add_to_module(crate::namespaces::Namespace::Type, name, def_id, span);

        let mut node = ModuleNode::new(def_id, name, Some(parent), visibility);
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

    fn collect_const(&mut self, c: &Const, span: Span, visibility: Visibility) {
        let def_id = self.next_def_id();
        let name = c.name.symbol;
        self.add_def(def_id, name, span, DefKind::Const, visibility);
        self.add_to_module(crate::namespaces::Namespace::Value, name, def_id, span);
    }

    fn collect_static(&mut self, s: &Static, span: Span, visibility: Visibility) {
        let def_id = self.next_def_id();
        let name = s.name.symbol;
        self.add_def(def_id, name, span, DefKind::Static, visibility);
        self.add_to_module(crate::namespaces::Namespace::Value, name, def_id, span);
    }

    fn collect_impl(&mut self, i: &Impl, span: Span, visibility: Visibility) {
        let def_id = self.next_def_id();
        // Impl blocks don't have a single name; use a synthetic name.
        let name = self.interner.get_or_intern("<impl>");
        self.add_def(def_id, name, span, DefKind::Impl, visibility);

        // Extract type name from self_ty
        let type_name = Self::extract_type_name(&i.self_ty);

        // Extract trait name if this is a trait impl
        let trait_name = i.trait_impl.as_ref().and_then(|path| {
            path.segments.last().map(|s| s.ident.symbol)
        });

        if let Some(type_name) = type_name {
            if let Some(trait_name) = trait_name {
                // Trait impl
                let key = (trait_name, type_name);
                self.trait_impls.entry(key).or_default().push(def_id);
            } else {
                // Inherent impl
                self.inherent_impls.entry(type_name).or_default().push(def_id);
            }
        }

        // Collect impl items
        let mut item_names = FxHashMap::default();
        for item in &i.items {
            let item_def_id = self.next_def_id();
            let (item_name, item_kind, item_span) = match &item.item {
                ImplItemKind::Method(fn_def) => {
                    (fn_def.name.symbol, DefKind::Fn, fn_def.name.span())
                }
                ImplItemKind::AssociatedType(ty_binding) => {
                    (ty_binding.name.symbol, DefKind::TypeAlias, ty_binding.span)
                }
                ImplItemKind::Constant(const_def) => {
                    (const_def.name.symbol, DefKind::Const, const_def.span)
                }
            };
            self.add_def(item_def_id, item_name, item_span, item_kind, item.visibility.clone());
            item_names.insert(item_name, item_def_id);
        }
        self.impl_item_names.insert(def_id, item_names);
    }

    fn extract_type_name(ty: &Type) -> Option<Symbol> {
        match &ty.kind {
            TypeKind::Named(path) => {
                path.segments.first().map(|s| s.ident.symbol)
            }
            TypeKind::Ref { ty, .. } => Self::extract_type_name(ty),
            _ => None,
        }
    }

    fn collect_use(&mut self, _u: &Use, span: Span, visibility: Visibility) {
        let def_id = self.next_def_id();
        let name = self.interner.get_or_intern("<use>");
        self.add_def(def_id, name, span, DefKind::Use, visibility);
        // Use items are resolved later during early resolution.
    }

    fn add_def(&mut self, def_id: DefId, name: Symbol, span: Span, kind: DefKind, visibility: Visibility) {
        self.definitions.insert(
            def_id,
            Definition {
                def_id,
                name,
                span,
                kind,
                parent: Some(self.current_module),
                visibility,
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
