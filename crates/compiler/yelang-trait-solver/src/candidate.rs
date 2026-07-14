/*! Candidate assembly for trait goals.
 *
 * Candidates are ways to prove a goal: param-env assumptions, user impls,
 * built-in rules, auto-trait derivations, etc.
 */

use yelang_util::DefId;

/// A candidate solution for a goal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub source: CandidateSource,
    // In a full implementation, this would carry nested goals,
    // constraints, etc.
}

/// Where a candidate came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CandidateSource {
    /// From a user-written impl block.
    Impl(DefId),
    /// From a param-env assumption (where clause).
    ParamEnv,
    /// From a built-in rule (Sized, Copy, etc.).
    Builtin,
    /// From an auto-trait derivation.
    AutoTrait,
    /// From a blanket impl.
    Blanket,
}
