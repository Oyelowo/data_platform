/*! Solver responses and certainty. */

pub use yelang_ty::canonical::{CanonicalResponse, Certainty, NoSolution, Response};

use yelang_ty::canonical::Canonical;
use yelang_ty::predicate::Predicate;

/// The result of evaluating a canonical goal.
pub type SolverResult<'tcx> = Result<CanonicalResponse<'tcx>, NoSolution>;

/// A nested goal with its source (for diagnostics and cycle tracking).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NestedGoal<'tcx> {
    pub source: GoalSource,
    pub goal: CanonicalGoal<'tcx>,
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
pub type CanonicalGoal<'tcx> = Canonical<'tcx, Predicate<'tcx>>;
