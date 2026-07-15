//! Resolution results for HIR paths.
//!
//! `Res` describes how a name was resolved: to a definition, a local variable,
//! a primitive type, etc.

use crate::ids::HirId;
use yelang_arena::DefId;

/// How a path was resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Res {
    /// Resolved to a definition.
    Def { def_id: DefId },
    /// Resolved to a local variable.
    Local { hir_id: HirId },
    /// Primitive type (`i32`, `bool`, etc.).
    PrimTy { ty: PrimTy },
    /// `Self` in an impl or trait.
    SelfTy { def_id: DefId },
    /// `self` parameter.
    SelfVal { def_id: DefId },
    /// Error recovery.
    Err,
}

/// Primitive type kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimTy {
    Int(IntTy),
    Float(FloatTy),
    Bool,
    Char,
    Str,
}

/// Integer primitive types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntTy {
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
}

/// Floating-point primitive types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatTy {
    F32,
    F64,
}

/// Re-export the real resolved crate from `yelang-resolve`.
pub use yelang_resolve::ResolvedCrate;
