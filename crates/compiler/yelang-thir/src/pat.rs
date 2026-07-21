//! THIR patterns.

use yelang_ast::Mutability;
use yelang_hir::hir::core::Lit;
use yelang_hir::res::Res;
use yelang_interner::Symbol;

use crate::ids::ThirPatId;

/// Kinds of THIR patterns.
#[derive(Debug, Clone)]
pub enum ThirPat {
    /// `_`
    Wild,
    /// Variable binding, optionally with a sub-pattern.
    Binding { name: Symbol, subpat: Option<ThirPatId> },
    /// Struct pattern: `Point { x, y }`.
    Struct {
        res: Res,
        fields: Vec<(Symbol, ThirPatId)>,
        rest: bool,
    },
    /// Tuple pattern: `(a, b)`.
    Tuple { pats: Vec<ThirPatId> },
    /// Tuple-struct pattern: `Some(x)`.
    TupleStruct { res: Res, pats: Vec<ThirPatId> },
    /// Reference pattern: `&pat` or `&mut pat`.
    Ref { mutability: Mutability, pat: ThirPatId },
    /// Path pattern (enum variant without data, or constant).
    Path { res: Res },
    /// Literal pattern.
    Lit { lit: Lit },
    /// Range pattern.
    Range {
        start: Option<ThirPatId>,
        end: Option<ThirPatId>,
        end_inclusive: bool,
    },
    /// Or pattern: `A | B`.
    Or { pats: Vec<ThirPatId> },
    /// Slice pattern: `[a, .., b]`.
    Slice {
        prefix: Vec<ThirPatId>,
        middle: Option<ThirPatId>,
        suffix: Vec<ThirPatId>,
    },
    /// Slice rest pattern: `..` inside a slice pattern.
    Rest,
    /// Error recovery.
    Err,
}
