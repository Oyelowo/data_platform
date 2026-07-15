use crate::{Ident, Literal, Path, RangeExpr};
use yelang_lexer::Span;

#[derive(Debug, Clone, PartialEq)]
pub struct Pattern {
    pub pattern: PatternKind,
    pub span: Span,
}

impl Pattern {
    pub fn span(&self) -> &Span {
        &self.span
    }
}

/// Patterns for destructuring and matching.
///
/// Used in let bindings, function parameters, and match expressions.
#[derive(Debug, Clone, PartialEq)]
pub enum PatternKind {
    /// An absent pattern, e.g. anonymous param: `fn f(i8)`.
    Absent,

    /// Variable binding: `x`, `mut y`.
    Binding {
        name: Ident,
        mutability: Mutability,
        subpattern: Option<Box<Pattern>>,
    },

    /// Wildcard pattern: `_`.
    Wildcard,

    /// Path patterns: `MyEnum::Variant`, `MY_CONSTANT`.
    Path(Path),

    /// Literal patterns: `42`, `"hello"`, `true`.
    Literal(Literal),

    /// Tuple patterns: `(x, y)`, `(head, .., tail)`.
    Tuple { patterns: Vec<Pattern> },

    /// Struct patterns: `Point { x, y }`, `User { name, .. }`.
    Struct {
        path: Path,
        fields: Vec<FieldPattern>,
        rest: bool,
    },

    /// Structural record patterns: `{ x, y: renamed, .. }`.
    Record {
        fields: Vec<FieldPattern>,
        rest: bool,
    },

    /// Tuple-struct patterns: `Enum::B(.., a)`.
    TupleStruct { path: Path, patterns: Vec<Pattern> },

    /// Slice patterns: `[x, y, z]`, `[first, .., last]`.
    Slice { patterns: Vec<Pattern> },

    /// Reference patterns: `&pat`, `&mut pat`.
    Ref { pattern: Box<Pattern>, is_mut: bool },

    /// Or patterns: `A | B | C`.
    Or(Vec<Pattern>),

    /// Slice/tuple rest pattern: `..` or slice rest binding `..rest`.
    Rest { name: Option<Ident> },

    /// Range patterns: `1..=10`, `..5`.
    Range(RangeExpr),

    /// Grouped pattern: `(pattern)`.
    Grouped(Box<Pattern>),

    /// Macro invocation in pattern position: `MyPat!()`.
    MacroInvocation(crate::expr::MacroInvocation),
}

/// A field in a struct pattern.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldPattern {
    pub name: Ident,
    pub pattern: Pattern,
    pub is_shorthand: bool,
    pub is_placeholder: bool,
}

/// Mutability qualifier for bindings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mutability {
    Immutable,
    Mutable,
}

/// Helper struct to break recursion and allow parsing patterns without top-level ORs.
/// Used for function/lambda parameters to avoid ambiguity with `|` delimiters.
#[derive(Debug, Clone, PartialEq)]
pub struct RestrictedPattern(pub Pattern);

/// Pattern element used inside slice patterns.
///
/// This exists to support `..rest` *only* inside slice patterns without
/// interfering with range patterns like `..X` at top-level.
#[derive(Debug, Clone, PartialEq)]
pub struct SlicePatternElement(pub Pattern);
