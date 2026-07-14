//! Patterns in HIR.

use yelang_interner::Symbol;
use yelang_lexer::Span;

use crate::ids::HirId;
use crate::res::Res;
use crate::hir::Lit;

/// A pattern node.
#[derive(Debug, Clone)]
pub struct Pat {
    pub hir_id: HirId,
    pub kind: PatKind,
    pub span: Span,
}

/// Kinds of patterns.
#[derive(Debug, Clone)]
pub enum PatKind {
    /// `_`
    Wild,
    /// Variable binding, optionally with a sub-pattern.
    Binding {
        mode: BindingMode,
        name: Symbol,
        subpat: Option<Box<Pat>>,
    },
    /// Struct pattern: `Point { x, y }`
    Struct {
        res: Res,
        fields: Vec<FieldPat>,
        rest: bool,
    },
    /// Tuple pattern: `(a, b)`
    Tuple { pats: Vec<Pat> },
    /// Tuple-struct pattern: `Some(x)`
    TupleStruct {
        res: Res,
        pats: Vec<Pat>,
    },
    /// Path pattern (enum variant without data, or constant).
    Path { res: Res },
    /// Literal pattern.
    Lit { lit: Lit },
    /// Range pattern.
    Range {
        start: Option<Box<Pat>>,
        end: Option<Box<Pat>>,
        end_inclusive: bool,
    },
    /// Or pattern: `A | B`
    Or { pats: Vec<Pat> },
    /// Slice pattern: `[a, .., b]`
    Slice {
        prefix: Vec<Pat>,
        middle: Option<Box<Pat>>,
        suffix: Vec<Pat>,
    },
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
    pub ident: crate::hir::Ident,
    pub pat: Pat,
    pub is_shorthand: bool,
    pub span: Span,
}
