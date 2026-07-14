//! Types in HIR.

use yelang_interner::Symbol;
use yelang_lexer::Span;

use crate::hir::FnSig;
use crate::res::Res;
use crate::hir::Lit;

/// A type node.
#[derive(Debug, Clone)]
pub struct Ty {
    pub kind: TyKind,
    pub span: Span,
}

/// Kinds of types.
#[derive(Debug, Clone)]
pub enum TyKind {
    /// Resolved path type.
    Path { res: Res },
    /// Tuple type: `(i32, bool)`
    Tuple { tys: Vec<Ty> },
    /// Array type: `[T; N]`
    Array { ty: Box<Ty>, len: Const },
    /// Slice type: `[T]`
    Slice { ty: Box<Ty> },
    /// Function pointer type.
    FnPtr { sig: Box<FnSig> },
    /// Anonymous struct type: `{ x: i32, y: i32 }`
    AnonStruct { fields: Vec<AnonField> },
    /// Type literal: `"pending" | "active"`
    TypeLit { variants: Vec<Lit> },
    /// Utility type: `Omit<T, K>`
    Utility { kind: UtilityKind, args: Vec<Ty> },
    /// Reference: `&T` or `&mut T`
    Ref { mutability: yelang_ast::Mutability, ty: Box<Ty> },
    /// Raw pointer: `*mut T` or `*const T`
    RawPtr { mutability: yelang_ast::Mutability, ty: Box<Ty> },
    /// Higher-ranked type: `for<T> fn(T) -> T`
    ForAll { params: Vec<crate::hir::GenericParam>, ty: Box<Ty> },
    /// Union type: `i32 | string | bool`
    Union { tys: Vec<Ty> },
    /// `impl Trait` opaque type.
    ImplTrait { path: Res },
    /// `dyn Trait` trait object type.
    DynTrait { path: Res },
    /// Type inference variable.
    Infer,
    /// Error recovery.
    Err,
}

/// A field in an anonymous struct type.
#[derive(Debug, Clone)]
pub struct AnonField {
    pub name: Symbol,
    pub ty: Ty,
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

/// A type-level constant (used for array lengths).
#[derive(Debug, Clone)]
pub struct Const {
    pub kind: ConstKind,
    pub span: Span,
}

/// Kinds of type-level constants.
#[derive(Debug, Clone)]
pub enum ConstKind {
    Lit { lit: Lit },
    Err,
}
