/*! Type inference errors. */

use std::fmt;

use yelang_interner::Symbol;
use yelang_ty::predicate::{ProjectionPredicate, TraitPredicate};
use yelang_ty::primitive::{FloatTy, IntTy};
use yelang_ty::ty::{ConstId, TyId, TyVid};

/// An error that occurred during type inference or unification.
#[derive(Clone, Debug, PartialEq)]
pub enum TypeError {
    /// Mismatched types: expected `expected`, found `found`.
    Mismatch { expected: TyId, found: TyId },
    /// Cyclic type: `?T = Vec<?T>`.
    CyclicTy(TyVid),
    /// Unsolved inference variable at the end of type checking.
    UnresolvedInferenceVariable(TyVid),
    /// Invalid projection: `<T as Trait>::Item` not found.
    ProjectionNotFound(ProjectionPredicate),
    /// Trait not implemented: `T: Trait` not satisfied.
    TraitNotImplemented(TraitPredicate),
    /// Ambiguous trait bound.
    AmbiguousTrait(TraitPredicate),
    /// Invalid field access.
    NoSuchField { ty: TyId, field: Symbol },
    /// Invalid method call.
    NoSuchMethod { ty: TyId, method: Symbol },
    /// Arity mismatch in call.
    ArgCount { expected: usize, found: usize },
    /// Generic argument count mismatch.
    GenericArgCount { expected: usize, found: usize },
    /// Generic argument kind mismatch at the given index (e.g. type vs const).
    GenericArgKindMismatch { index: usize },
    /// Integral type mismatch (e.g. `i32` vs `i64`).
    IntMismatch { expected: IntTy, found: IntTy },
    /// Floating-point type mismatch (e.g. `f32` vs `f64`).
    FloatMismatch { expected: FloatTy, found: FloatTy },
    /// Constant value mismatch.
    ConstMismatch {
        expected: ConstId,
        found: ConstId,
    },
    /// Trait reference mismatch (e.g. different trait in a projection).
    TraitRefMismatch {
        expected: yelang_ty::predicate::TraitRef,
        found: yelang_ty::predicate::TraitRef,
    },
    /// Existential predicate mismatch in trait objects.
    ExistentialMismatch {
        expected: yelang_ty::existential::ExistentialPredicate,
        found: yelang_ty::existential::ExistentialPredicate,
    },
    /// Custom error message (avoid in unification paths).
    Custom(String),
}

impl fmt::Display for TypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeError::Mismatch { expected, found } => {
                write!(
                    f,
                    "type mismatch: expected `{:?}`, found `{:?}`",
                    expected, found
                )
            }
            TypeError::CyclicTy(vid) => write!(f, "cyclic type: `?T{}`", vid.0),
            TypeError::UnresolvedInferenceVariable(vid) => {
                write!(f, "unresolved inference variable: `?T{}`", vid.0)
            }
            TypeError::ProjectionNotFound(p) => {
                write!(f, "projection not found: `{:?}`", p)
            }
            TypeError::TraitNotImplemented(p) => {
                write!(f, "trait not implemented: `{:?}`", p)
            }
            TypeError::AmbiguousTrait(p) => {
                write!(f, "ambiguous trait bound: `{:?}`", p)
            }
            TypeError::NoSuchField { ty, field } => {
                write!(f, "no field `{:?}` on type `{:?}`", field.as_usize(), ty)
            }
            TypeError::NoSuchMethod { ty, method } => {
                write!(f, "no method `{:?}` on type `{:?}`", method.as_usize(), ty)
            }
            TypeError::ArgCount { expected, found } => {
                write!(
                    f,
                    "argument count mismatch: expected {}, found {}",
                    expected, found
                )
            }
            TypeError::GenericArgCount { expected, found } => {
                write!(
                    f,
                    "generic argument count mismatch: expected {}, found {}",
                    expected, found
                )
            }
            TypeError::GenericArgKindMismatch { index } => {
                write!(f, "generic argument kind mismatch at index {}", index)
            }
            TypeError::IntMismatch { expected, found } => {
                write!(
                    f,
                    "integer type mismatch: expected `{:?}`, found `{:?}`",
                    expected, found
                )
            }
            TypeError::FloatMismatch { expected, found } => {
                write!(
                    f,
                    "float type mismatch: expected `{:?}`, found `{:?}`",
                    expected, found
                )
            }
            TypeError::ConstMismatch { expected, found } => {
                write!(
                    f,
                    "const mismatch: expected `{:?}`, found `{:?}`",
                    expected, found
                )
            }
            TypeError::TraitRefMismatch { expected, found } => {
                write!(
                    f,
                    "trait ref mismatch: expected `{:?}`, found `{:?}`",
                    expected, found
                )
            }
            TypeError::ExistentialMismatch { expected, found } => {
                write!(
                    f,
                    "existential mismatch: expected `{:?}`, found `{:?}`",
                    expected, found
                )
            }
            TypeError::Custom(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for TypeError {}
