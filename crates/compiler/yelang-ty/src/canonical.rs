/*! Canonicalization — removing inference variables for caching.
 *
 * A canonical value has all inference vars replaced by bound vars.
 * This makes goals cacheable: `Vec<?T>: Clone` and `Vec<?U>: Clone`
 * both canonicalize to `exists<T> Vec<T>: Clone`.
 */

use crate::list::List;
use crate::ty::{PlaceholderType, UniverseIndex};

/// A canonicalized value: all inference vars are bound.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Canonical<'tcx, V> {
    pub value: V,
    pub max_universe: UniverseIndex,
    pub variables: CanonicalVarKinds<'tcx>,
    pub _marker: std::marker::PhantomData<&'tcx ()>,
}

impl<'tcx, V> Canonical<'tcx, V> {
    pub fn new(value: V, max_universe: UniverseIndex, variables: CanonicalVarKinds<'tcx>) -> Self {
        Self {
            value,
            max_universe,
            variables,
            _marker: std::marker::PhantomData,
        }
    }
}

/// An interned list of canonical variable kinds.
pub type CanonicalVarKinds<'tcx> = List<CanonicalVarKind>;

/// The kind of a canonical variable.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CanonicalVarKind {
    /// An existential type variable: `exists<T>`.
    Ty(CanonicalTyVarKind),
    /// A placeholder type variable: `!T_N`.
    PlaceholderTy(PlaceholderType),
    /// An integral variable.
    Int,
    /// A float variable.
    Float,
    /// A const variable.
    Const,
}

/// The kind of an existential type variable.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CanonicalTyVarKind {
    General(UniverseIndex),
    Int,
    Float,
}

/// Certainty of a canonical response.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Certainty {
    /// The goal is provable.
    Yes,
    /// The goal may be provable (ambiguous).
    Maybe,
    /// The goal is not provable.
    No,
}

/// An error indicating no solution exists.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct NoSolution;

impl std::fmt::Display for NoSolution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "no solution")
    }
}

impl std::error::Error for NoSolution {}

/// The result of a canonical goal evaluation.
pub type CanonicalResponse<'tcx> = Canonical<'tcx, Response<'tcx>>;

/// A response from the trait solver.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Response<'tcx> {
    pub certainty: Certainty,
    pub goals: List<Canonical<'tcx, crate::predicate::Predicate<'tcx>>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn certainty_ordering() {
        assert_ne!(Certainty::Yes, Certainty::No);
        assert_ne!(Certainty::Maybe, Certainty::Yes);
    }

    #[test]
    fn canonical_var_kinds() {
        let kind = CanonicalVarKind::Ty(CanonicalTyVarKind::General(UniverseIndex(0)));
        assert!(matches!(kind, CanonicalVarKind::Ty(_)));
    }
}
