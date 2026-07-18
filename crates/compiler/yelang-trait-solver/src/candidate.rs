/*! Candidate assembly for trait goals.
 *
 * Candidates are ways to prove a goal: param-env assumptions, user impls,
 * built-in rules, auto-trait derivations, and blanket impls.
 */

use yelang_ty::predicate::Predicate;

use crate::solver_ctx::{BuiltinTraitKind, ImplInfo};

/// A candidate solution for a goal.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub source: CandidateSource,
}

/// Where a candidate came from.
#[derive(Clone, Debug)]
pub enum CandidateSource {
    /// From a user-written impl block.
    UserImpl(ImplInfo),
    /// From a param-env assumption (where clause).
    ParamEnv(Predicate),
    /// From a built-in rule (`Sized`, `Copy`, `Clone`, ...).
    Builtin(BuiltinTraitKind),
    /// From an auto-trait derivation.
    AutoTrait,
    /// From a blanket impl.
    Blanket,
}
