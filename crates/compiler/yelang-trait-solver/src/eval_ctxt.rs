/*! EvalCtxt — the solver engine.
 *
 * EvalCtxt is the core of the next-generation trait solver. It evaluates
 * goals recursively, using the search graph for cycle detection and caching.
 */

use yelang_ty::canonical::{Canonical, Certainty, NoSolution, Response};
use yelang_ty::list::List;
use yelang_ty::predicate::{Predicate, TraitPredicate};
use yelang_ty::ty::UniverseIndex;

use crate::candidate::{Candidate, CandidateSource};
use crate::goal::Goal;
use crate::response::{CanonicalGoal, CanonicalResponse, NestedGoal, SolverResult};
use crate::search_graph::SearchGraph;

/// The evaluation context for the trait solver.
pub struct EvalCtxt<'tcx> {
    /// The search graph for cycle detection and caching.
    search_graph: SearchGraph<'tcx>,
    /// Currently accumulated nested goals.
    nested_goals: Vec<NestedGoal<'tcx>>,
    /// The highest universe index visible.
    max_universe: UniverseIndex,
    /// Whether the result is tainted.
    tainted: Result<(), NoSolution>,
}

impl<'tcx> EvalCtxt<'tcx> {
    pub fn new() -> Self {
        Self {
            search_graph: SearchGraph::new(),
            nested_goals: Vec::new(),
            max_universe: UniverseIndex(0),
            tainted: Ok(()),
        }
    }

    /// Entry point: evaluate a root goal.
    pub fn evaluate_root_goal(&mut self, goal: Goal<'tcx>) -> SolverResult<'tcx> {
        let canonical_goal = self.canonicalize_goal(goal);
        self.evaluate_canonical_goal(canonical_goal)
    }

    /// Evaluate a canonical goal.
    fn evaluate_canonical_goal(
        &mut self,
        canonical_goal: CanonicalGoal<'tcx>,
    ) -> SolverResult<'tcx> {
        // 1. Check the cache.
        if let Some(entry) = self.search_graph.lookup_cache(&canonical_goal) {
            return Ok(entry.result.clone());
        }

        // 2. Check for cycles.
        if let Some(stack_index) = self.search_graph.is_in_stack(&canonical_goal) {
            // Cycle detected. For coinductive traits (auto traits), we can
            // succeed provisionally. For inductive traits, we fail.
            // For simplicity, we treat all cycles as ambiguous here.
            self.search_graph.mark_coinductive(stack_index);
            return Ok(self.make_response(Certainty::Maybe));
        }

        // 3. Push onto stack and evaluate.
        self.search_graph.push(canonical_goal);
        let result = self.compute_goal(canonical_goal);
        self.search_graph.pop();

        // 4. Cache the result.
        if let Ok(ref response) = result {
            self.search_graph
                .insert_cache(canonical_goal, response.clone());
        }

        result
    }

    /// The main solver logic: dispatch on predicate kind.
    fn compute_goal(&mut self, goal: CanonicalGoal<'tcx>) -> SolverResult<'tcx> {
        // In a full implementation, we'd instantiate the canonical goal
        // and match on the predicate. For now, we sketch the structure.
        let predicate = goal.value;

        match predicate {
            Predicate::Trait(trait_pred) => self.compute_trait_goal(goal, trait_pred),
            Predicate::Projection(proj_pred) => self.compute_projection_goal(goal, proj_pred),
            Predicate::TypeOutlives(_) => {
                // No-op in Yelang (no lifetimes).
                Ok(self.make_response(Certainty::Yes))
            }
            Predicate::ConstEvaluatable(_) => {
                // TODO: const evaluation
                Ok(self.make_response(Certainty::Yes))
            }
        }
    }

    /// Compute a trait goal.
    fn compute_trait_goal(
        &mut self,
        _goal: CanonicalGoal<'tcx>,
        trait_pred: TraitPredicate<'tcx>,
    ) -> SolverResult<'tcx> {
        // 1. Assemble all possible candidates.
        let candidates = self.assemble_candidates(trait_pred);

        if candidates.is_empty() {
            return Err(NoSolution);
        }

        // 2. Evaluate each candidate in isolation.
        let mut responses = Vec::new();
        for _candidate in candidates {
            // In a full implementation, we'd evaluate each candidate in a probe.
            // For now, we treat all candidates as potentially successful.
            responses.push(self.make_response(Certainty::Yes));
        }

        // 3. Try to merge responses.
        self.merge_responses(&responses)
    }

    /// Compute a projection (associated type normalization) goal.
    fn compute_projection_goal(
        &mut self,
        _goal: CanonicalGoal<'tcx>,
        _proj_pred: yelang_ty::predicate::ProjectionPredicate<'tcx>,
    ) -> SolverResult<'tcx> {
        // TODO: implement normalization
        Ok(self.make_response(Certainty::Yes))
    }

    /// Assemble candidates for a trait goal.
    fn assemble_candidates(&mut self, _trait_pred: TraitPredicate<'tcx>) -> Vec<Candidate> {
        let mut candidates = Vec::new();

        // TODO: ParamEnv candidate
        candidates.push(Candidate {
            source: CandidateSource::ParamEnv,
        });

        // TODO: Built-in candidate (Sized, Copy, Clone)
        candidates.push(Candidate {
            source: CandidateSource::Builtin,
        });

        // TODO: User impl candidates
        // TODO: Auto-trait candidates

        candidates
    }

    /// Merge multiple responses into one.
    fn merge_responses(&mut self, responses: &[CanonicalResponse<'tcx>]) -> SolverResult<'tcx> {
        match responses.len() {
            0 => Err(NoSolution),
            1 => Ok(responses[0].clone()),
            _ => {
                // Multiple candidates: check if they agree.
                // If all are Yes with the same constraints, return Yes.
                // If there's ambiguity, return Maybe.
                // For now, we return Maybe to be safe.
                Ok(self.make_response(Certainty::Maybe))
            }
        }
    }

    /// Canonicalize a goal.
    fn canonicalize_goal(&self, goal: Goal<'tcx>) -> CanonicalGoal<'tcx> {
        // TODO: proper canonicalization (replace inference vars with bound vars)
        Canonical::new(goal.predicate, self.max_universe, List::empty())
    }

    /// Create a response with the given certainty.
    fn make_response(&self, certainty: Certainty) -> CanonicalResponse<'tcx> {
        Canonical::new(
            Response {
                certainty,
                goals: List::empty(),
            },
            self.max_universe,
            List::empty(),
        )
    }
}

impl<'tcx> Default for EvalCtxt<'tcx> {
    fn default() -> Self {
        Self::new()
    }
}
