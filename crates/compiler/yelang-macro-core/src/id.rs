use yelang_arena::{Arena, ArenaKey, Id};

pub use yelang_arena::CrateId;

/// A unique identifier for a single macro expansion invocation.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExpnId(ArenaKey);

impl ExpnId {
    pub fn from_arena_key(key: ArenaKey) -> Self {
        Self(key)
    }

    pub fn as_arena_key(self) -> ArenaKey {
        self.0
    }
}

/// A hygiene context: a chain of macro expansion marks.
///
/// Uses a raw integer ID so it can be serialized across the proc-macro server
/// boundary and reconstructed on the other side without depending on a shared
/// arena allocator.
pub type SyntaxContextId = yelang_arena::Id<yelang_arena::tags::TagSyntaxContext>;

/// A declared macro definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MacroDefId(ArenaKey);

impl MacroDefId {
    pub fn from_arena_key(key: ArenaKey) -> Self {
        Self(key)
    }

    pub fn as_arena_key(self) -> ArenaKey {
        self.0
    }
}

/// Controls how identifiers in a given hygiene context resolve.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Transparency {
    /// Fully hygienic: resolves only in the macro definition scope.
    #[default]
    Opaque,
    /// Fully unhygienic: resolves in the call-site scope.
    Transparent,
    /// Mixed: types/items are definition-site, local bindings are call-site.
    Mixed,
}

/// Data associated with an `ExpnId`.
#[derive(Debug, Clone)]
pub struct ExpnData {
    pub parent: ExpnId,
    pub call_site: yelang_lexer::Span,
    pub def_site: yelang_lexer::Span,
    pub kind: ExpnKind,
    pub desc: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpnKind {
    Root,
    MacroRules,
    Macro,
    ProcMacro,
    Comptime,
    AstPass,
}

/// Data associated with a `SyntaxContextId`.
#[derive(Debug, Clone)]
pub struct SyntaxContextData {
    pub parent: Option<SyntaxContextId>,
    pub outer_expn: Option<ExpnId>,
    pub transparency: Transparency,
}

impl SyntaxContextData {
    pub fn root() -> Self {
        Self {
            parent: None,
            outer_expn: None,
            transparency: Transparency::Opaque,
        }
    }
}

/// Data associated with a `MacroDefId`.
#[derive(Debug, Clone)]
pub struct MacroDefData {
    pub name: yelang_interner::Symbol,
    pub span: yelang_lexer::Span,
    pub kind: MacroKind,
    pub defining_crate: CrateId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacroKind {
    Declarative,
    FunctionLike,
    Attribute,
    Derive,
    Comptime,
}

/// Arena types for macro hygiene/expansion data.
pub type ExpnArena = Arena<ExpnData>;
pub type SyntaxContextArena = Arena<SyntaxContextData>;
pub type MacroDefArena = Arena<MacroDefData>;

/// Tag type for `TokenId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TagToken {}

/// A unique identifier for a single token.
///
/// Used by the macro/token API for fine-grained provenance tracking.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TokenId(Id<TagToken>);

impl TokenId {
    pub fn fresh() -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(1);
        let raw = COUNTER.fetch_add(1, Ordering::SeqCst);
        Self(Id::new(raw))
    }
}
