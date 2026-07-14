//! YeLang prelude injection.
//!
//! Following Rust's model (RFC 1560, RFC 0503), every module implicitly imports
//! a set of standard items unless opted out with `@no_implicit_prelude`.
//!
//! Prelude names are checked as a final fallback during name resolution, meaning
//! they can be shadowed by any local definition, import, or ancestor module item.

use yelang_ast::Visibility;
use yelang_interner::{Interner, Symbol};
use yelang_lexer::Span;
use yelang_util::{DefId, FxHashMap};

use crate::{
    def_collector::{DefCollector, DefKind, Definition},
    module_tree::ModuleNode,
    namespaces::Namespace,
};

/// Built-in prelude items injected into every module.
///
/// These are placeholder definitions that resolve successfully during name
/// resolution. Downstream phases (type checking, codegen) must recognize them
/// as built-in types and provide their actual semantics.
#[derive(Debug, Clone)]
pub struct Prelude {
    /// Map from namespace to (name symbol -> DefId) for prelude items.
    pub items: FxHashMap<Namespace, FxHashMap<Symbol, DefId>>,
    /// The underlying definitions for all prelude items.
    pub definitions: FxHashMap<DefId, Definition>,
}

impl Prelude {
    /// Build the standard YeLang prelude.
    pub fn new(interner: &Interner, next_def_id: &mut u32) -> Self {
        let mut items: FxHashMap<Namespace, FxHashMap<Symbol, DefId>> = FxHashMap::default();
        let mut definitions: FxHashMap<DefId, Definition> = FxHashMap::default();

        let mut add = |name: &str, kind: DefKind, ns: Namespace| {
            let symbol = interner.get_or_intern(name);
            let def_id = DefId::new(*next_def_id);
            *next_def_id += 1;

            let def = Definition {
                def_id,
                name: symbol,
                span: Span::default(),
                kind,
                parent: None,
                visibility: Visibility::Public(Span::default()),
            };

            definitions.insert(def_id, def);
            items
                .entry(ns)
                .or_insert_with(FxHashMap::default)
                .insert(symbol, def_id);

            def_id
        };

        // Core data types (type namespace)
        add("Option", DefKind::Enum, Namespace::Type);
        add("Option", DefKind::Enum, Namespace::Value); // enum variants live in value ns too

        add("Result", DefKind::Enum, Namespace::Type);
        add("Result", DefKind::Enum, Namespace::Value);

        add("Vec", DefKind::Struct, Namespace::Type);
        add("Vec", DefKind::Struct, Namespace::Value);

        add("String", DefKind::TypeAlias, Namespace::Type);
        add("String", DefKind::TypeAlias, Namespace::Value);

        add("Box", DefKind::Struct, Namespace::Type);
        add("Box", DefKind::Struct, Namespace::Value);

        // Common traits (type namespace)
        add("Copy", DefKind::Trait, Namespace::Type);
        add("Clone", DefKind::Trait, Namespace::Type);
        add("Default", DefKind::Trait, Namespace::Type);
        add("Debug", DefKind::Trait, Namespace::Type);
        add("Display", DefKind::Trait, Namespace::Type);
        add("PartialEq", DefKind::Trait, Namespace::Type);
        add("Eq", DefKind::Trait, Namespace::Type);
        add("PartialOrd", DefKind::Trait, Namespace::Type);
        add("Ord", DefKind::Trait, Namespace::Type);
        add("Iterator", DefKind::Trait, Namespace::Type);
        add("IntoIterator", DefKind::Trait, Namespace::Type);
        add("Send", DefKind::Trait, Namespace::Type);
        add("Sync", DefKind::Trait, Namespace::Type);
        add("Sized", DefKind::Trait, Namespace::Type);

        // Common functions (value namespace)
        add("drop", DefKind::Fn, Namespace::Value);
        add("Some", DefKind::EnumVariant, Namespace::Value);
        add("None", DefKind::EnumVariant, Namespace::Value);
        add("Ok", DefKind::EnumVariant, Namespace::Value);
        add("Err", DefKind::EnumVariant, Namespace::Value);

        Self { items, definitions }
    }

    /// Inject prelude items into a module node unless it has opted out.
    pub fn inject_into(&self, module: &mut ModuleNode) {
        for (ns, name_map) in &self.items {
            for (symbol, def_id) in name_map {
                // Only insert if the module doesn't already define this name
                // in this namespace. This ensures explicit definitions shadow
                // the prelude, matching Rust semantics.
                if module.get_item(*ns, *symbol).is_none() {
                    module.add_item(*ns, *symbol, *def_id);
                }
            }
        }
    }
}

/// Check whether an item's attributes contain `@no_implicit_prelude`.
pub fn has_no_implicit_prelude(
    attributes: &[yelang_ast::Attribute],
    no_implicit_prelude_sym: Symbol,
) -> bool {
    attributes.iter().any(|attr| {
        attr.path
            .first()
            .map(|ident| ident.symbol == no_implicit_prelude_sym)
            .unwrap_or(false)
    })
}


