//! Patterns in HIR.

use yelang_interner::Symbol;
use yelang_lexer::Span;

use crate::hir::core::Lit;
use crate::ids::PatId;
use crate::res::Res;

/// Kinds of patterns.
#[derive(Debug, Clone)]
pub enum Pat {
    /// `_`
    Wild,
    /// Variable binding, optionally with a sub-pattern.
    Binding {
        mode: BindingMode,
        name: Symbol,
        subpat: Option<PatId>,
    },
    /// Struct pattern: `Point { x, y }`
    Struct {
        res: Res,
        fields: Vec<FieldPat>,
        rest: bool,
    },
    /// Tuple pattern: `(a, b)`
    Tuple { pats: Vec<PatId> },
    /// Tuple-struct pattern: `Some(x)`
    TupleStruct { res: Res, pats: Vec<PatId> },
    /// Reference pattern: `&pat` or `&mut pat`.
    Ref { pat: PatId, mutability: yelang_ast::Mutability },
    /// Path pattern (enum variant without data, or constant).
    Path { res: Res },
    /// Literal pattern.
    Lit { lit: Lit },
    /// Range pattern.
    Range {
        start: Option<PatId>,
        end: Option<PatId>,
        end_inclusive: bool,
    },
    /// Or pattern: `A | B`
    Or { pats: Vec<PatId> },
    /// Slice pattern: `[a, .., b]`
    Slice {
        prefix: Vec<PatId>,
        middle: Option<PatId>,
        suffix: Vec<PatId>,
    },
    /// Slice rest pattern: `..` or `..rest` inside a slice pattern.
    Rest { name: Option<Symbol> },
    /// Error recovery.
    Err,
}

/// Binding mode for a pattern variable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindingMode {
    ByValue,
    ByRef { mutability: yelang_ast::Mutability },
}

/// A field in a struct pattern.
#[derive(Debug, Clone)]
pub struct FieldPat {
    pub ident: crate::hir::core::Ident,
    pub pat: PatId,
    pub is_shorthand: bool,
    pub span: Span,
}
