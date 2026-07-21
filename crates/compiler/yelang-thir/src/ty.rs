//! THIR type references.
//!
//! THIR does not define its own type system; it points to the canonical,
//! interned types produced by the type checker.

use yelang_ty::ty::TyId;

/// A type in THIR is just a wrapper around the type checker's `TyId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThirTyId(pub TyId);
