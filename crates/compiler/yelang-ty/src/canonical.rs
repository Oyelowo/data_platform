/*! Canonicalization — removing inference variables for caching.
 *
 * A canonical value has all inference vars replaced by bound vars.
 * This makes goals cacheable: `Vec<?T>: Clone` and `Vec<?U>: Clone`
 * both canonicalize to `exists<T> Vec<T>: Clone`.
 */

use crate::list::List;
use crate::primitive::{FloatTy, IntTy, UintTy};
use crate::ty::{ConstId, PlaceholderType, TyId, UniverseIndex};

/// A canonicalized value: all inference vars are bound.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Canonical<V> {
    pub value: V,
    pub max_universe: UniverseIndex,
    pub variables: CanonicalVarKinds,
}

impl<V> Canonical<V> {
    pub fn new(value: V, max_universe: UniverseIndex, variables: CanonicalVarKinds) -> Self {
        Self {
            value,
            max_universe,
            variables,
        }
    }
}

/// An interned list of canonical variable kinds.
pub type CanonicalVarKinds = List<CanonicalVarKind>;

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
pub type CanonicalResponse = Canonical<Response>;

/// A response from the trait solver.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Response {
    pub certainty: Certainty,
    pub goals: List<Canonical<crate::predicate::Predicate>>,
    /// Inferred values for the canonical variables of the original goal.
    pub var_values: List<CanonicalVarValue>,
}

/// The value of a canonical variable in a response.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CanonicalVarValue {
    /// The solver learned nothing about this variable.
    Unknown,
    /// The variable resolved to a concrete type.
    Ty(TyId),
    /// The variable resolved to a concrete const.
    Const(ConstId),
    /// The variable resolved to a concrete signed integer type.
    Int(IntTy),
    /// The variable resolved to a concrete unsigned integer type.
    Uint(UintTy),
    /// The variable resolved to a concrete float type.
    Float(FloatTy),
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
