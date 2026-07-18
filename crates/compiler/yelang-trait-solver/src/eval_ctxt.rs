/*! EvalCtxt — the solver engine.
 *
 * `EvalCtxt` is the core of the next-generation trait solver. It evaluates
 * goals recursively, using the search graph for cycle detection and caching.
 *
 * The implementation follows the recursive solver design from
 * `rustc_next_trait_solver` and Chalk:
 *
 * - Goals are canonicalized before lookup/caching.
 * - The search graph is both the DFS stack and the global cache.
 * - Cycles are detected via stack membership.
 * - Coinductive cycles iterate to a `Yes` fixpoint.
 * - Inductive cycles and depth exhaustion return `Maybe` (overflow).
 * - Candidates are evaluated in isolated `InferCtxt::probe` snapshots.
 */

use yelang_infer::InferCtxt;
use yelang_ty::canonical::{Canonical, Certainty, NoSolution, Response};
use yelang_ty::generic::{GenericArg, Substitution};
use yelang_ty::interner::Interner;
use yelang_ty::list::List;
use yelang_ty::predicate::{Predicate, TraitPredicate, TraitRef};
use yelang_ty::subst::substitute;
use yelang_ty::ty::UniverseIndex;

use crate::builtin::{is_clone, is_copy, is_sized};
use crate::candidate::{Candidate, CandidateSource};
use crate::canonicalize::canonicalize_goal;
use crate::goal::Goal;
use crate::response::{CanonicalGoal, CanonicalResponse, NestedGoal, SolverResult};
use crate::search_graph::SearchGraph;
use crate::solver_ctx::{BuiltinTraitKind, SolverCtxt};

/// Default recursion depth budget for a root goal.
///
/// Large enough for realistic trait hierarchies, small enough to terminate
/// quickly on pathological inputs.
const DEFAULT_MAX_DEPTH: usize = 64;

/// The evaluation context for the trait solver.
pub struct EvalCtxt<'tcx, C: SolverCtxt<'tcx>> {
    /// The interner for constructing and interning types.
    interner: &'tcx Interner<'tcx>,
    /// The solver context: trait definitions, impls, built-ins.
    tcx: &'tcx C,
    /// The inference context for speculative unification.
    infcx: InferCtxt<'tcx>,
    /// The search graph for cycle detection and caching.
    search_graph: SearchGraph<'tcx>,
    /// Currently accumulated nested goals.
    ///
    /// In Phase 4 the solver proves nested goals eagerly, so this is mostly
    /// a placeholder for the lazy nested-goal work in Phase 5/6.
    nested_goals: Vec<NestedGoal<'tcx>>,
    /// The highest universe index visible.
    max_universe: UniverseIndex,
    /// Remaining recursion depth budget.
    available_depth: usize,
    /// Whether the result is tainted.
    tainted: Result<(), NoSolution>,
}

impl<'tcx, C: SolverCtxt<'tcx>> EvalCtxt<'tcx, C> {
    pub fn new(interner: &'tcx Interner<'tcx>, tcx: &'tcx C) -> Self {
        Self::with_max_depth(interner, tcx, DEFAULT_MAX_DEPTH)
    }

    pub fn with_max_depth(interner: &'tcx Interner<'tcx>, tcx: &'tcx C, max_depth: usize) -> Self {
        Self {
            interner,
            tcx,
            infcx: InferCtxt::new(),
            search_graph: SearchGraph::new(),
            nested_goals: Vec::new(),
            max_universe: UniverseIndex(0),
            available_depth: max_depth,
            tainted: Ok(()),
        }
    }

    /// Entry point: evaluate a root goal.
    pub fn evaluate_root_goal(&mut self, goal: Goal<'tcx>) -> SolverResult<'tcx> {
        let canonical_goal =
            canonicalize_goal(goal, self.interner, &mut self.infcx, self.max_universe);
        self.evaluate_canonical_goal(canonical_goal)
    }

    /// Borrow the inference context, e.g. to inspect resolved type variables
    /// after a successful root evaluation.
    pub fn infcx(&self) -> &InferCtxt<'tcx> {
        &self.infcx
    }

    /// Mutable borrow of the inference context, useful for creating fresh
    /// inference variables before passing a goal into the solver.
    pub fn infcx_mut(&mut self) -> &mut InferCtxt<'tcx> {
        &mut self.infcx
    }

    // -----------------------------------------------------------------------
    // Core recursive loop
    // -----------------------------------------------------------------------

    /// Evaluate a canonical goal.
    fn evaluate_canonical_goal(
        &mut self,
        canonical_goal: CanonicalGoal<'tcx>,
    ) -> SolverResult<'tcx> {
        // 1. Check the cache, respecting the remaining depth budget.
        if let Some(entry) = self
            .search_graph
            .lookup_cache(&canonical_goal, self.available_depth)
        {
            return Ok(entry.result.clone());
        }

        // 2. Check for cycles.
        if let Some(stack_index) = self.search_graph.is_in_stack(&canonical_goal) {
            return self.handle_cycle(stack_index, canonical_goal);
        }

        // 3. Push onto stack and evaluate.
        self.search_graph.push(canonical_goal, self.available_depth);

        let snapshot = self.infcx.snapshot();
        let mut result = self.compute_goal(canonical_goal);

        // 4. Coinductive fixpoint iteration.
        let depth = self.search_graph.depth();
        let is_coinductive = depth > 0
            && self
                .search_graph
                .stack_entry(depth - 1)
                .map(|e| e.coinductive)
                .unwrap_or(false);

        if is_coinductive {
            let mut iterations = 0;
            const MAX_FIXPOINT_ITERATIONS: usize = 8;

            while result.as_ref().map(|r| r.value.certainty) != Ok(Certainty::Yes)
                && iterations < MAX_FIXPOINT_ITERATIONS
            {
                self.infcx.rollback_to(snapshot);
                self.search_graph
                    .set_provisional(depth - 1, self.make_response(Certainty::Yes));
                result = self.compute_goal(canonical_goal);
                iterations += 1;
            }
        }

        if result.as_ref().map(|r| r.value.certainty) != Ok(Certainty::Yes) {
            self.infcx.rollback_to(snapshot);
        }

        let entry = self.search_graph.pop().expect("stack should not be empty");

        // 5. Cache the result if this goal was not merely a cycle participant.
        if result.is_ok() && !entry.has_cycle {
            self.search_graph.insert_cache(
                canonical_goal,
                result.clone().unwrap(),
                self.available_depth,
            );
        }

        result
    }

    /// Handle a goal that is already on the evaluation stack.
    fn handle_cycle(
        &mut self,
        stack_index: usize,
        canonical_goal: CanonicalGoal<'tcx>,
    ) -> SolverResult<'tcx> {
        let is_coinductive = self.is_coinductive_goal(&canonical_goal.value);

        self.search_graph.mark_cycle(stack_index);

        if is_coinductive {
            self.search_graph.mark_coinductive(stack_index);
            // If there is already a provisional result, use it.
            if let Some(entry) = self.search_graph.stack_entry(stack_index) {
                if let Some(ref provisional) = entry.provisional {
                    return Ok(provisional.clone());
                }
            }
            self.search_graph
                .set_provisional(stack_index, self.make_response(Certainty::Yes));
            Ok(self.make_response(Certainty::Yes))
        } else {
            Ok(self.make_response(Certainty::Maybe))
        }
    }

    /// True if the given goal may be solved coinductively.
    fn is_coinductive_goal(&self, goal: &Goal<'tcx>) -> bool {
        match goal.predicate {
            Predicate::Trait(trait_pred) => {
                if let Some(kind) = self.tcx.builtin_kind(trait_pred.trait_ref.def_id) {
                    return matches!(kind, BuiltinTraitKind::Sized);
                }
                self.tcx
                    .trait_info(trait_pred.trait_ref.def_id)
                    .map(|info| info.is_auto)
                    .unwrap_or(false)
            }
            Predicate::WellFormed(_) => true,
            _ => false,
        }
    }

    /// The main solver logic: dispatch on predicate kind.
    fn compute_goal(&mut self, goal: CanonicalGoal<'tcx>) -> SolverResult<'tcx> {
        let instantiated = self.instantiate_canonical_goal(goal);

        match instantiated.predicate {
            Predicate::Trait(trait_pred) => self.compute_trait_goal(instantiated, trait_pred),
            Predicate::Projection(_proj_pred) => {
                // TODO(Phase 5): normalize projection and equate.
                Ok(self.make_response(Certainty::Yes))
            }
            Predicate::NormalizesTo(_norm_pred) => {
                // TODO(Phase 5): normalize associated type and equate.
                Ok(self.make_response(Certainty::Yes))
            }
            Predicate::WellFormed(wf_pred) => {
                // TODO(Phase 5): structural well-formedness.
                if matches!(wf_pred.ty.kind(), yelang_ty::ty::TyKind::Error) {
                    Err(NoSolution)
                } else {
                    Ok(self.make_response(Certainty::Yes))
                }
            }
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

    /// Instantiate a canonical goal, replacing bound variables with fresh
    /// inference variables.
    fn instantiate_canonical_goal(&mut self, goal: CanonicalGoal<'tcx>) -> Goal<'tcx> {
        crate::instantiate::instantiate(goal, self.interner, &mut self.infcx)
    }

    // -----------------------------------------------------------------------
    // Trait goals
    // -----------------------------------------------------------------------

    /// Compute a trait goal.
    fn compute_trait_goal(
        &mut self,
        goal: Goal<'tcx>,
        trait_pred: TraitPredicate<'tcx>,
    ) -> SolverResult<'tcx> {
        let candidates = self.assemble_candidates(goal, trait_pred);

        if candidates.is_empty() {
            return Err(NoSolution);
        }

        let mut yes_count = 0;
        let mut maybe_count = 0;
        let mut selected: Option<Candidate<'tcx>> = None;

        for candidate in candidates {
            let response = self.probe(|this| this.try_candidate(goal, &candidate));
            match response {
                Ok(ref r) if r.value.certainty == Certainty::Yes => {
                    yes_count += 1;
                    selected = Some(candidate.clone());
                }
                Ok(ref r) if r.value.certainty == Certainty::Maybe => {
                    maybe_count += 1;
                }
                _ => {}
            }
        }

        match yes_count {
            0 if maybe_count > 0 => Ok(self.make_response(Certainty::Maybe)),
            0 => Err(NoSolution),
            1 => {
                // Commit the unique successful candidate.
                if let Some(candidate) = selected {
                    self.try_candidate(goal, &candidate)
                } else {
                    Err(NoSolution)
                }
            }
            _ => {
                // Overlap: more than one impl applies.
                self.tainted = Err(NoSolution);
                Ok(self.make_response(Certainty::Maybe))
            }
        }
    }

    /// Assemble candidates for a trait goal.
    fn assemble_candidates(
        &mut self,
        goal: Goal<'tcx>,
        trait_pred: TraitPredicate<'tcx>,
    ) -> Vec<Candidate<'tcx>> {
        let mut candidates = Vec::new();

        // Param-env assumptions.
        for &pred in goal.param_env.caller_bounds.iter() {
            if let Predicate::Trait(assumption) = pred {
                if assumption.polarity == trait_pred.polarity
                    && assumption.trait_ref.def_id == trait_pred.trait_ref.def_id
                {
                    candidates.push(Candidate {
                        source: CandidateSource::ParamEnv(pred),
                    });
                }
            }
        }

        // Built-in rules.
        if let Some(kind) = self.tcx.builtin_kind(trait_pred.trait_ref.def_id) {
            candidates.push(Candidate {
                source: CandidateSource::Builtin(kind),
            });
        }

        // User-written impls.
        for impl_info in self.tcx.impls_for_trait(trait_pred.trait_ref.def_id) {
            candidates.push(Candidate {
                source: CandidateSource::UserImpl(impl_info.clone()),
            });
        }

        // Auto-trait and blanket stubs.
        if let Some(info) = self.tcx.trait_info(trait_pred.trait_ref.def_id) {
            if info.is_auto {
                candidates.push(Candidate {
                    source: CandidateSource::AutoTrait,
                });
            }
        }

        candidates
    }

    /// Evaluate a single candidate. On success this commits the unifications
    /// and nested-goal proofs performed by the candidate.
    fn try_candidate(
        &mut self,
        goal: Goal<'tcx>,
        candidate: &Candidate<'tcx>,
    ) -> SolverResult<'tcx> {
        match &candidate.source {
            CandidateSource::ParamEnv(assumption) => {
                self.try_param_env_candidate(goal, *assumption)
            }
            CandidateSource::Builtin(kind) => self.try_builtin_candidate(goal, *kind),
            CandidateSource::UserImpl(impl_info) => self.try_user_impl_candidate(goal, impl_info),
            CandidateSource::AutoTrait => self.try_auto_trait_candidate(goal),
            CandidateSource::Blanket => Err(NoSolution),
        }
    }

    fn try_param_env_candidate(
        &mut self,
        goal: Goal<'tcx>,
        assumption: Predicate<'tcx>,
    ) -> SolverResult<'tcx> {
        let Predicate::Trait(assumption) = assumption else {
            return Err(NoSolution);
        };
        let goal_pred = match goal.predicate {
            Predicate::Trait(tp) => tp,
            _ => return Err(NoSolution),
        };

        if assumption.polarity != goal_pred.polarity {
            return Err(NoSolution);
        }
        if assumption.trait_ref.def_id != goal_pred.trait_ref.def_id {
            return Err(NoSolution);
        }

        self.infcx
            .eq_generic_args(&assumption.trait_ref.args, &goal_pred.trait_ref.args)
            .map_err(|_| NoSolution)?;

        Ok(self.make_response(Certainty::Yes))
    }

    fn try_builtin_candidate(
        &mut self,
        goal: Goal<'tcx>,
        kind: BuiltinTraitKind,
    ) -> SolverResult<'tcx> {
        let Predicate::Trait(trait_pred) = goal.predicate else {
            return Err(NoSolution);
        };
        let self_ty = self.trait_self_ty(&trait_pred.trait_ref)?;

        let satisfied = match kind {
            BuiltinTraitKind::Sized => is_sized(self_ty.kind()),
            BuiltinTraitKind::Copy => is_copy(self_ty.kind()),
            BuiltinTraitKind::Clone => is_clone(self_ty.kind()),
        };

        if satisfied {
            Ok(self.make_response(Certainty::Yes))
        } else {
            Err(NoSolution)
        }
    }

    fn try_user_impl_candidate(
        &mut self,
        goal: Goal<'tcx>,
        impl_info: &crate::solver_ctx::ImplInfo<'tcx>,
    ) -> SolverResult<'tcx> {
        let goal_pred = match goal.predicate {
            Predicate::Trait(tp) => tp,
            _ => return Err(NoSolution),
        };

        if impl_info.trait_ref.def_id != goal_pred.trait_ref.def_id {
            return Err(NoSolution);
        }

        // Create fresh inference variables for each impl generic parameter.
        let mut subst_args = Vec::with_capacity(impl_info.generic_param_count);
        for _ in 0..impl_info.generic_param_count {
            subst_args.push(GenericArg::Type(self.infcx.new_ty_var(self.interner)));
        }
        let subst = Substitution::from_args(subst_args);

        let impl_trait_ref = substitute(self.interner, impl_info.trait_ref, &subst);
        let impl_predicates: Vec<_> = impl_info
            .predicates
            .iter()
            .map(|&p| substitute(self.interner, p, &subst))
            .collect();

        self.infcx
            .eq_trait_refs(&impl_trait_ref, &goal_pred.trait_ref)
            .map_err(|_| NoSolution)?;

        let mut certainty = Certainty::Yes;
        for pred in impl_predicates {
            let response = self.add_goal(Goal::new(goal.param_env, pred))?;
            if response.value.certainty == Certainty::Maybe {
                certainty = Certainty::Maybe;
            }
        }

        Ok(self.make_response(certainty))
    }

    fn try_auto_trait_candidate(&mut self, _goal: Goal<'tcx>) -> SolverResult<'tcx> {
        // TODO(Phase 5): derive auto traits from ADT fields / tuple elems.
        Ok(self.make_response(Certainty::Maybe))
    }

    /// Extract the `Self` type from a trait reference.
    ///
    /// In Yelang the first generic argument of a `TraitRef` is always `Self`.
    fn trait_self_ty(
        &self,
        trait_ref: &TraitRef<'tcx>,
    ) -> Result<yelang_ty::ty::Ty<'tcx>, NoSolution> {
        trait_ref
            .args
            .iter()
            .next()
            .and_then(|arg| arg.expect_type_checked())
            .ok_or(NoSolution)
    }

    // -----------------------------------------------------------------------
    // Nested goals and probes
    // -----------------------------------------------------------------------

    /// Add a nested goal, consuming one level of recursion depth.
    fn add_goal(&mut self, goal: Goal<'tcx>) -> SolverResult<'tcx> {
        if self.available_depth == 0 {
            return Ok(self.make_response(Certainty::Maybe));
        }

        let canonical_goal =
            canonicalize_goal(goal, self.interner, &mut self.infcx, self.max_universe);

        self.available_depth -= 1;
        let result = self.evaluate_canonical_goal(canonical_goal);
        self.available_depth += 1;

        result
    }

    /// Execute `f` in a speculative snapshot, rolling back all inference
    /// state on return.
    fn probe<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let snapshot = self.infcx.snapshot();
        let nested_goals_snapshot = self.nested_goals.len();
        let tainted_snapshot = self.tainted.clone();
        let max_universe_snapshot = self.max_universe;

        let result = f(self);

        self.infcx.rollback_to(snapshot);
        self.nested_goals.truncate(nested_goals_snapshot);
        self.tainted = tainted_snapshot;
        self.max_universe = max_universe_snapshot;

        result
    }

    // -----------------------------------------------------------------------
    // Responses
    // -----------------------------------------------------------------------

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

// -----------------------------------------------------------------------------
// Helper extension
// -----------------------------------------------------------------------------

trait ExpectTypeExt<'tcx> {
    fn expect_type_checked(self) -> Option<yelang_ty::ty::Ty<'tcx>>;
}

impl<'tcx> ExpectTypeExt<'tcx> for GenericArg<'tcx> {
    fn expect_type_checked(self) -> Option<yelang_ty::ty::Ty<'tcx>> {
        match self {
            GenericArg::Type(ty) => Some(ty),
            GenericArg::Const(_) => None,
        }
    }
}
