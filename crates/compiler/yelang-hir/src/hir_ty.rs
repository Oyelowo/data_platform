//! Types in HIR.

use yelang_interner::Symbol;
use yelang_lexer::Span;

use crate::hir::FnSig;
use crate::hir::Lit;
use crate::ids::{BodyId, ExprId, TyId};
use crate::res::Res;
use yelang_ast::Ident;

/// Kinds of types.
#[derive(Debug, Clone)]
pub enum Ty {
    /// Resolved path type, optionally with generic arguments.
    ///
    /// Examples:
    /// - `i32` -> `Path { res: PrimTy(Int(I32)), args: [] }`
    /// - `Vec<T>` -> `Path { res: Def(Vec), args: [Type(T)] }`
    Path { res: Res, args: Vec<GenericArg> },
    /// Tuple type: `(i32, bool)`
    Tuple { tys: Vec<TyId> },
    /// Array type: `[T; N]`
    Array { ty: TyId, len: Const },
    /// Slice type: `[T]`
    Slice { ty: TyId },
    /// Function pointer type.
    FnPtr { sig: Box<FnSig> },
    /// Anonymous struct type: `{ x: i32, y: i32 }`
    AnonStruct { fields: Vec<AnonField> },
    /// Type literal: `"pending" | "active"`
    TypeLit { variants: Vec<Lit> },
    /// Utility type: `Omit<T, K>`
    Utility { kind: UtilityKind, args: Vec<TyId> },
    /// `typeof expr` type.
    TypeOf { expr: ExprId },
    /// Reference: `&T` or `&mut T`
    Ref {
        mutability: yelang_ast::Mutability,
        ty: TyId,
    },
    /// Raw pointer: `*mut T` or `*const T`
    RawPtr {
        mutability: yelang_ast::Mutability,
        ty: TyId,
    },
    /// Higher-ranked type: `for<T> fn(T) -> T`
    ForAll {
        params: Vec<crate::hir::GenericParam>,
        ty: TyId,
    },
    /// Union type: `i32 | string | bool`
    Union { tys: Vec<TyId> },
    /// `impl Trait` opaque type.
    ImplTrait { path: Res },
    /// `dyn Trait` trait object type.
    DynTrait { path: Res },
    /// The never type `!`.
    Never,
    /// Type inference variable.
    Infer,
    /// A type that was omitted in source (e.g. un-annotated let) and must be inferred.
    Missing,
    /// Error recovery.
    Err,
}

/// A generic argument in a path.
#[derive(Debug, Clone)]
pub enum GenericArg {
    /// Type argument: `T` in `Vec<T>`.
    Type(TyId),
    /// Const argument: `N` in `[T; N]` or `Foo<N>`.
    Const(Const),
    /// Associated type binding: `Item = i32`.
    AssocBinding { name: Ident, ty: TyId },
}

/// A field in an anonymous struct type.
#[derive(Debug, Clone)]
pub struct AnonField {
    pub name: Symbol,
    pub ty: TyId,
}

/// Utility type kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UtilityKind {
    Omit,
    Pick,
    ReturnType,
    Params,
    Partial,
    Required,
}

/// A type-level constant (used for array lengths, enum discriminants, and const generics).
#[derive(Debug, Clone)]
pub struct Const {
    pub kind: ConstKind,
    pub span: Span,
}

/// Kinds of type-level constants.
#[derive(Debug, Clone)]
pub enum ConstKind {
    /// A literal constant, e.g. `42` or `"foo"`.
    Lit { lit: Lit },
    /// A constant expression that must be evaluated by const-eval.
    /// The body has no parameters.
    Expr { body: BodyId },
    /// Error recovery.
    Err,
}
