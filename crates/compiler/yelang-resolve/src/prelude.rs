//! YeLang prelude definitions.
//!
//! Following Rust's model (RFC 1560, RFC 0503), every module implicitly has
//! access to a set of standard items unless opted out with `@no_implicit_prelude`.
//!
//! Prelude names are checked as a final fallback during name resolution, meaning
//! they can be shadowed by any local definition, import, or ancestor module item.
//! They are intentionally *not* inserted into module namespace tables, so they
//! are not re-exported by glob imports and cannot be accessed through qualified
//! paths such as `crate::Option`.

use yelang_arena::{DefId, FxHashMap, IndexVec};
use yelang_ast::Visibility;
use yelang_interner::{Interner, Symbol};
use yelang_lexer::Span;

use crate::{
    def_collector::{DefKind, Definition},
    lang_items::{LangItem, LangItems},
    namespaces::Namespace,
};

/// Built-in prelude items injected into every module.
///
/// The prelude does *not* own `Definition` values.  They live in the shared
/// `IndexVec<DefId, Definition>` arena owned by `DefCollector` so that every
/// definition in the crate has a single, dense allocation discipline.
#[derive(Debug, Clone)]
pub struct Prelude {
    /// Map from namespace to (name symbol -> DefId) for prelude items.
    pub items: FxHashMap<Namespace, FxHashMap<Symbol, DefId>>,
    /// Enum variant mappings for prelude enums (`Option`, `Result`).
    pub enum_variants: FxHashMap<DefId, FxHashMap<Symbol, DefId>>,
}

impl Prelude {
    /// Build the standard YeLang prelude.
    ///
    /// Definitions are allocated directly into the supplied `definitions` arena.
    /// The returned `LangItems` registry contains all lang items discovered in
    /// the prelude (e.g. `Box`, `Formatter`).
    pub fn new(
        interner: &Interner,
        definitions: &mut IndexVec<DefId, Definition>,
    ) -> (Self, LangItems, FxHashMap<DefId, FxHashMap<Symbol, DefId>>) {
        let mut items: FxHashMap<Namespace, FxHashMap<Symbol, DefId>> = FxHashMap::default();
        let mut lang_items = LangItems::new();

        let mut add =
            |name: &str, kind: DefKind, ns: Namespace, lang_item: Option<LangItem>| -> DefId {
                let symbol = interner.get_or_intern(name);
                let def_id = definitions.push(Definition {
                    // Patched to the real key after allocation.
                    def_id: DefId::new(1),
                    name: symbol,
                    span: Span::default(),
                    kind,
                    parent: None,
                    visibility: Visibility::Public(Span::default()),
                    lang_item,
                });
                definitions[def_id].def_id = def_id;

                if let Some(li) = lang_item {
                    lang_items.insert(li, def_id);
                }
                items
                    .entry(ns)
                    .or_insert_with(FxHashMap::default)
                    .insert(symbol, def_id);

                def_id
            };

        // Core data types (type namespace)
        add("Option", DefKind::Enum, Namespace::Type, None);
        add("Option", DefKind::Enum, Namespace::Value, None); // enum variants live in value ns too

        add("Result", DefKind::Enum, Namespace::Type, None);
        add("Result", DefKind::Enum, Namespace::Value, None);

        add("Vec", DefKind::Struct, Namespace::Type, None);
        add("Vec", DefKind::Struct, Namespace::Value, None);

        add("Array", DefKind::Struct, Namespace::Type, Some(LangItem::Array));
        add("Array", DefKind::Struct, Namespace::Value, Some(LangItem::Array));

        add("String", DefKind::TypeAlias, Namespace::Type, None);
        add("String", DefKind::TypeAlias, Namespace::Value, None);

        add("Box", DefKind::Struct, Namespace::Type, Some(LangItem::Box));
        add(
            "Box",
            DefKind::Struct,
            Namespace::Value,
            Some(LangItem::Box),
        );

        // Core formatter type used by derived `Debug` impls.
        add(
            "Formatter",
            DefKind::Struct,
            Namespace::Type,
            Some(LangItem::Formatter),
        );
        add(
            "Formatter",
            DefKind::Struct,
            Namespace::Value,
            Some(LangItem::Formatter),
        );

        // Common traits (type namespace)
        add(
            "Copy",
            DefKind::Trait,
            Namespace::Type,
            Some(LangItem::Copy),
        );
        add(
            "Clone",
            DefKind::Trait,
            Namespace::Type,
            Some(LangItem::Clone),
        );
        add(
            "Default",
            DefKind::Trait,
            Namespace::Type,
            Some(LangItem::Default),
        );
        add(
            "Debug",
            DefKind::Trait,
            Namespace::Type,
            Some(LangItem::Debug),
        );
        add(
            "Display",
            DefKind::Trait,
            Namespace::Type,
            Some(LangItem::Display),
        );
        add(
            "PartialEq",
            DefKind::Trait,
            Namespace::Type,
            Some(LangItem::PartialEq),
        );
        add("Eq", DefKind::Trait, Namespace::Type, None); // Eq is not a lang item in Rust
        add(
            "PartialOrd",
            DefKind::Trait,
            Namespace::Type,
            Some(LangItem::PartialOrd),
        );
        add(
            "Ord",
            DefKind::Trait,
            Namespace::Type,
            Some(LangItem::OrdTrait),
        );
        add(
            "Iterator",
            DefKind::Trait,
            Namespace::Type,
            Some(LangItem::Iterator),
        );
        add(
            "IntoIterator",
            DefKind::Trait,
            Namespace::Type,
            Some(LangItem::IntoIterator),
        );
        add(
            "Send",
            DefKind::Trait,
            Namespace::Type,
            Some(LangItem::Send),
        );
        add(
            "Sync",
            DefKind::Trait,
            Namespace::Type,
            Some(LangItem::Sync),
        );
        add(
            "Sized",
            DefKind::Trait,
            Namespace::Type,
            Some(LangItem::Sized),
        );

        // Common functions (value namespace)
        add("drop", DefKind::Fn, Namespace::Value, Some(LangItem::Drop));
        add("len", DefKind::Fn, Namespace::Value, Some(LangItem::Len));
        add("count", DefKind::Fn, Namespace::Value, Some(LangItem::Count));
        let some_sym = interner.get_or_intern("Some");
        let none_sym = interner.get_or_intern("None");
        let ok_sym = interner.get_or_intern("Ok");
        let err_sym = interner.get_or_intern("Err");
        let some_id = add("Some", DefKind::EnumVariant, Namespace::Value, None);
        let none_id = add("None", DefKind::EnumVariant, Namespace::Value, None);
        let ok_id = add("Ok", DefKind::EnumVariant, Namespace::Value, None);
        let err_id = add("Err", DefKind::EnumVariant, Namespace::Value, None);

        let option_sym = interner.get_or_intern("Option");
        let result_sym = interner.get_or_intern("Result");

        let mut prelude = Self {
            items,
            enum_variants: FxHashMap::default(),
        };

        // Register prelude enum variant mappings so that downstream passes can
        // synthesize `Option::Some`, `Result::Ok`, etc., without re-resolving.
        let option_id = prelude
            .items
            .get(&Namespace::Type)
            .and_then(|m| m.get(&option_sym))
            .copied();
        let result_id = prelude
            .items
            .get(&Namespace::Type)
            .and_then(|m| m.get(&result_sym))
            .copied();

        if let Some(id) = option_id {
            let mut variants = FxHashMap::default();
            variants.insert(some_sym, some_id);
            variants.insert(none_sym, none_id);
            prelude.enum_variants.insert(id, variants);
        }
        if let Some(id) = result_id {
            let mut variants = FxHashMap::default();
            variants.insert(ok_sym, ok_id);
            variants.insert(err_sym, err_id);
            prelude.enum_variants.insert(id, variants);
        }

        let enum_variants = prelude.enum_variants.clone();
        (prelude, lang_items, enum_variants)
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
