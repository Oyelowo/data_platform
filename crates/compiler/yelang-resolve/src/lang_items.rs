//! YeLang language-item registry.
//!
//! certain types, traits,
//! and functions are "known to the compiler" via the `@lang("...")` decorator.
//!
//! The registry is built during DefCollection by scanning for `@lang` attributes
//! and by seeding synthetic entries for primitive types.  Downstream passes
//! (type checking, codegen, MIR lowering) query `tcx.lang_items()` (or our
//! equivalent) instead of hard-coding symbol strings.
//!
//! Lang items are loaded lazily: the compiler emits an error only when a
//! required lang item is needed but not found.

use yelang_arena::{DefId, FxHashMap, IndexVec};
use yelang_interner::{Interner, Symbol};

/// A language item — an item that the compiler knows about by name rather than
/// by path.
///
/// This enum is intentionally exhaustive.  Adding a new lang item requires:
/// 1. Adding a variant here.
/// 2. Adding the string mapping in `LangItem::from_name`.
/// 3. Seeding or allowing `@lang` registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LangItem {
    // ------------------------------------------------------------------------
    // Primitive types
    // ------------------------------------------------------------------------
    I8,
    I16,
    I32,
    I64,
    I128,
    Isize,
    U8,
    U16,
    U32,
    U64,
    U128,
    Usize,
    F32,
    F64,
    Bool,
    Char,
    Str,

    // ------------------------------------------------------------------------
    // Marker traits (auto-traits the compiler reasons about)
    // ------------------------------------------------------------------------
    Copy,
    Send,
    Sync,
    Sized,

    // ------------------------------------------------------------------------
    // Operator traits (overloadable via lang-item markers)
    // ------------------------------------------------------------------------
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Neg,
    Not,
    Deref,
    DerefMut,
    DerefTarget,
    Index,
    IndexMut,
    EqTrait, // `==`
    PartialEq,
    OrdTrait, // `<` / `>` ordering
    PartialOrd,

    // ------------------------------------------------------------------------
    // Other fundamental traits
    // ------------------------------------------------------------------------
    Drop,
    Clone,
    Default,
    Debug,
    Display,
    Iterator,
    IntoIterator,

    // ------------------------------------------------------------------------
    // Special types
    // ------------------------------------------------------------------------
    Box,         // `Box<T>` — owned heap allocation
    PhantomData, // variance / dropck marker
    Formatter,   // `fmt::Formatter` used by derived `Debug` impls

    // ------------------------------------------------------------------------
    // Panic / unwinding
    // ------------------------------------------------------------------------
    Panic,
    PanicBoundsCheck,
    DropInPlace,

    // ------------------------------------------------------------------------
    // Entry point / runtime
    // ------------------------------------------------------------------------
    Start,
}

impl LangItem {
    /// Convert the canonical lang-item string (e.g. `"sized"`) to a `LangItem`.
    pub fn from_name(name: &str) -> Option<Self> {
        use LangItem::*;
        Some(match name {
            // Primitives
            "i8" => I8,
            "i16" => I16,
            "i32" => I32,
            "i64" => I64,
            "i128" => I128,
            "isize" => Isize,
            "u8" => U8,
            "u16" => U16,
            "u32" => U32,
            "u64" => U64,
            "u128" => U128,
            "usize" => Usize,
            "f32" => F32,
            "f64" => F64,
            "bool" => Bool,
            "char" => Char,
            "str" => Str,

            // Marker traits
            "copy" => Copy,
            "send" => Send,
            "sync" => Sync,
            "sized" => Sized,

            // Operators
            "add" => Add,
            "sub" => Sub,
            "mul" => Mul,
            "div" => Div,
            "rem" => Rem,
            "bitand" => BitAnd,
            "bitor" => BitOr,
            "bitxor" => BitXor,
            "shl" => Shl,
            "shr" => Shr,
            "neg" => Neg,
            "not" => Not,
            "deref" => Deref,
            "deref_mut" => DerefMut,
            "deref_target" => DerefTarget,
            "index" => Index,
            "index_mut" => IndexMut,
            "eq" => EqTrait,
            "partial_eq" => PartialEq,
            "ord" => OrdTrait,
            "partial_ord" => PartialOrd,

            // Other traits
            "drop" => Drop,
            "clone" => Clone,
            "default" => Default,
            "debug" => Debug,
            "display" => Display,
            "iterator" => Iterator,
            "into_iterator" => IntoIterator,

            // Special types
            "owned_box" => Box,
            "phantom_data" => PhantomData,
            "formatter" => Formatter,

            // Panic / runtime
            "panic" => Panic,
            "panic_bounds_check" => PanicBoundsCheck,
            "drop_in_place" => DropInPlace,
            "start" => Start,

            _ => return None,
        })
    }

    /// Return the canonical string name for this lang item.
    pub fn name(self) -> &'static str {
        use LangItem::*;
        match self {
            I8 => "i8",
            I16 => "i16",
            I32 => "i32",
            I64 => "i64",
            I128 => "i128",
            Isize => "isize",
            U8 => "u8",
            U16 => "u16",
            U32 => "u32",
            U64 => "u64",
            U128 => "u128",
            Usize => "usize",
            F32 => "f32",
            F64 => "f64",
            Bool => "bool",
            Char => "char",
            Str => "str",
            Copy => "copy",
            Send => "send",
            Sync => "sync",
            Sized => "sized",
            Add => "add",
            Sub => "sub",
            Mul => "mul",
            Div => "div",
            Rem => "rem",
            BitAnd => "bitand",
            BitOr => "bitor",
            BitXor => "bitxor",
            Shl => "shl",
            Shr => "shr",
            Neg => "neg",
            Not => "not",
            Deref => "deref",
            DerefMut => "deref_mut",
            DerefTarget => "deref_target",
            Index => "index",
            IndexMut => "index_mut",
            EqTrait => "eq",
            PartialEq => "partial_eq",
            OrdTrait => "ord",
            PartialOrd => "partial_ord",
            Drop => "drop",
            Clone => "clone",
            Default => "default",
            Debug => "debug",
            Display => "display",
            Iterator => "iterator",
            IntoIterator => "into_iterator",
            Box => "owned_box",
            PhantomData => "phantom_data",
            Formatter => "formatter",
            Panic => "panic",
            PanicBoundsCheck => "panic_bounds_check",
            DropInPlace => "drop_in_place",
            Start => "start",
        }
    }

    /// Human-readable description for diagnostics.
    pub fn description(self) -> &'static str {
        use LangItem::*;
        match self {
            I8 | I16 | I32 | I64 | I128 | Isize | U8 | U16 | U32 | U64 | U128 | Usize | F32
            | F64 | Bool | Char | Str => "primitive type",
            Copy | Send | Sync | Sized => "marker trait",
            Add | Sub | Mul | Div | Rem | BitAnd | BitOr | BitXor | Shl | Shr | Neg | Not
            | Deref | DerefMut | Index | IndexMut | EqTrait | PartialEq | OrdTrait | PartialOrd => {
                "operator trait"
            }
            Drop | Clone | Default | Debug | Display | Iterator | IntoIterator => "standard trait",
            DerefTarget => "associated type",
            Box | PhantomData | Formatter => "special type",
            Panic | PanicBoundsCheck | DropInPlace => "panic/unwinding item",
            Start => "runtime entry point",
        }
    }
}

/// Registry mapping each `LangItem` to its `DefId`.
///
/// Built during DefCollection and owned by `Resolver`.
#[derive(Debug, Clone, Default)]
pub struct LangItems {
    map: FxHashMap<LangItem, DefId>,
}

impl LangItems {
    pub fn new() -> Self {
        Self {
            map: FxHashMap::default(),
        }
    }

    /// Register a lang item.  Returns the previous `DefId` if one existed.
    pub fn insert(&mut self, item: LangItem, def_id: DefId) -> Option<DefId> {
        self.map.insert(item, def_id)
    }

    /// Look up a lang item.
    pub fn get(&self, item: LangItem) -> Option<DefId> {
        self.map.get(&item).copied()
    }

    /// Iterate over all registered lang items.
    pub fn iter(&self) -> impl Iterator<Item = (LangItem, DefId)> + '_ {
        self.map.iter().map(|(&k, &v)| (k, v))
    }

    /// True if the given lang item has been registered.
    pub fn contains(&self, item: LangItem) -> bool {
        self.map.contains_key(&item)
    }

    /// Convenience: get the DefId for a lang item by its string name.
    pub fn get_by_name(&self, name: &str) -> Option<DefId> {
        LangItem::from_name(name).and_then(|li| self.get(li))
    }
}

/// Scan an attribute list for `@lang("...")` and return the lang-item name.
pub fn extract_lang_item_name(
    attributes: &[yelang_ast::Attribute],
    interner: &Interner,
) -> Option<(LangItem, Symbol)> {
    for attr in attributes {
        let attr_name = attr.path.first().map(|id| interner.resolve(&id.symbol))?;
        if attr_name != "lang" {
            continue;
        }
        let lang_str = match &attr.args {
            yelang_ast::AttributeArgs::Positional(exprs) => {
                exprs.first().and_then(|e| expr_to_string(e, interner))
            }
            _ => None,
        };
        if let Some(s) = lang_str {
            if let Some(li) = LangItem::from_name(&s) {
                return Some((li, interner.get_or_intern(&s)));
            }
        }
    }
    None
}

fn expr_to_string(expr: &yelang_ast::Expr, interner: &Interner) -> Option<String> {
    use yelang_ast::ExprKind;
    match &expr.kind {
        ExprKind::Path(path) if path.segments.len() == 1 => {
            Some(interner.resolve(&path.segments[0].ident.symbol).to_string())
        }
        ExprKind::Literal(yelang_ast::Literal::Str(s)) => {
            Some(interner.resolve(&s.value).to_string())
        }
        _ => None,
    }
}

/// Produce a `LangItems` registry pre-populated with all primitive types.
///
/// This is the principled replacement for ad-hoc `seed_primitives()`.
/// Each primitive is allocated directly into the shared `definitions` arena and
/// registered as a lang item.  The returned `Vec<(DefId, Namespace)>` should be
/// added to the root module namespace by the caller.
pub fn seed_primitive_lang_items(
    interner: &Interner,
    definitions: &mut IndexVec<DefId, crate::def_collector::Definition>,
) -> (LangItems, Vec<(DefId, crate::namespaces::Namespace)>) {
    use crate::def_collector::{DefKind, Definition};
    use crate::namespaces::Namespace;
    use yelang_ast::Visibility;
    use yelang_lexer::Span;

    let mut registry = LangItems::new();
    let mut to_add = Vec::new();

    let primitives: &[(LangItem, DefKind, Namespace)] = &[
        // Integer primitives
        (LangItem::I8, DefKind::TypeAlias, Namespace::Type),
        (LangItem::I16, DefKind::TypeAlias, Namespace::Type),
        (LangItem::I32, DefKind::TypeAlias, Namespace::Type),
        (LangItem::I64, DefKind::TypeAlias, Namespace::Type),
        (LangItem::I128, DefKind::TypeAlias, Namespace::Type),
        (LangItem::Isize, DefKind::TypeAlias, Namespace::Type),
        (LangItem::U8, DefKind::TypeAlias, Namespace::Type),
        (LangItem::U16, DefKind::TypeAlias, Namespace::Type),
        (LangItem::U32, DefKind::TypeAlias, Namespace::Type),
        (LangItem::U64, DefKind::TypeAlias, Namespace::Type),
        (LangItem::U128, DefKind::TypeAlias, Namespace::Type),
        (LangItem::Usize, DefKind::TypeAlias, Namespace::Type),
        // Float primitives
        (LangItem::F32, DefKind::TypeAlias, Namespace::Type),
        (LangItem::F64, DefKind::TypeAlias, Namespace::Type),
        // Other primitives
        (LangItem::Bool, DefKind::TypeAlias, Namespace::Type),
        (LangItem::Char, DefKind::TypeAlias, Namespace::Type),
        (LangItem::Str, DefKind::TypeAlias, Namespace::Type),
    ];

    for &(lang_item, kind, ns) in primitives {
        let name = interner.get_or_intern(lang_item.name());
        let def_id = definitions.push(Definition {
            // Patched to the real key after allocation.
            def_id: DefId::new(1),
            name,
            span: Span::default(),
            kind,
            parent: None,
            visibility: Visibility::Public(Span::default()),
            lang_item: Some(lang_item),
        });
        definitions[def_id].def_id = def_id;

        registry.insert(lang_item, def_id);
        to_add.push((def_id, ns));
    }

    (registry, to_add)
}
