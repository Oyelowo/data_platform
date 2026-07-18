/*! Solver responses and certainty. */

pub use yelang_ty::canonical::{CanonicalResponse, Certainty, NoSolution, Response};

use yelang_ty::canonical::Canonical;

use crate::goal::Goal;

/// The result of evaluating a canonical goal.
pub type SolverResult = Result<CanonicalResponse, NoSolution>;

/// A nested goal with its source (for diagnostics and cycle tracking).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NestedGoal {
    pub source: GoalSource,
    pub goal: CanonicalGoal,
}

/// Where a nested goal came from.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum GoalSource {
    /// Required for correctness.
    Impl,
    /// From a where clause on an impl.
    WhereClause,
    /// From a bound on a type parameter.
    Bound,
    /// From projection normalization.
    Normalize,
    /// From a coercion.
    Coerce,
}

/// A canonicalized goal.
///
/// The whole goal (param-env + predicate) is canonicalized so that the solver
/// cache distinguishes goals with different available assumptions.
pub type CanonicalGoal = Canonical<Goal>;
