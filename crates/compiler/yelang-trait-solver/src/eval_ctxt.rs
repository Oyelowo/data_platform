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

use yelang_infer::{ConstVarValue, FloatVarValue, InferCtxt, IntVarValue, TypeVarValue};
use yelang_ty::canonical::{
    Canonical, CanonicalVarKinds, CanonicalVarValue, Certainty, NoSolution, Response,
};
use yelang_ty::generic::{GenericArg, Substitution};
use yelang_ty::interner::Interner;
use yelang_ty::list::List;
use yelang_ty::predicate::{ParamEnv, Predicate, TraitPredicate, TraitRef};
use yelang_ty::subst::substitute;
use yelang_ty::ty::{InferTy, ProjectionTy, Ty, TyId, UniverseIndex};

use crate::instantiate::CanonicalVarMapping;

use crate::builtin::{is_clone, is_copy, is_sized};
use crate::candidate::{Candidate, CandidateSource};
use crate::canonicalize::canonicalize_goal;
use crate::goal::Goal;
use crate::response::{CanonicalGoal, CanonicalResponse, NestedGoal, SolverResult};
use crate::search_graph::SearchGraph;
use crate::solver_ctx::{BuiltinTraitKind, ImplInfo, SolverCtxt};

/// Default recursion depth budget for a root goal.
const DEFAULT_MAX_DEPTH: usize = 64;

/// The evaluation context for the trait solver.
pub struct EvalCtxt<'a, C: SolverCtxt> {
    /// The interner for constructing and interning types.
    interner: &'a Interner,
    /// The solver context: trait definitions, impls, built-ins.
    tcx: &'a C,
    /// The inference context for speculative unification.
    infcx: InferCtxt,
    /// The search graph for cycle detection and caching.
    search_graph: SearchGraph,
    /// Currently accumulated nested goals.
    nested_goals: Vec<NestedGoal>,
    /// The highest universe index visible.
    max_universe: UniverseIndex,
    /// Remaining recursion depth budget.
    available_depth: usize,
    /// Whether the result is tainted.
    tainted: Result<(), NoSolution>,
    /// Canonical variables of the current goal.
    canonical_variables: CanonicalVarKinds,
    /// Mapping from canonical variable index to the solver inference variable
    /// created for it.
    canonical_var_map: Vec<CanonicalVarMapping>,
}

impl<'a, C: SolverCtxt> EvalCtxt<'a, C> {
    pub fn new(interner: &'a Interner, tcx: &'a C) -> Self {
        Self::with_max_depth(interner, tcx, DEFAULT_MAX_DEPTH)
    }

    pub fn with_max_depth(interner: &'a Interner, tcx: &'a C, max_depth: usize) -> Self {
        Self {
            interner,
            tcx,
            infcx: InferCtxt::new(),
            search_graph: SearchGraph::new(),
            nested_goals: Vec::new(),
            max_universe: UniverseIndex(0),
            available_depth: max_depth,
            tainted: Ok(()),
            canonical_variables: List::empty(),
            canonical_var_map: Vec::new(),
        }
    }

    /// Entry point: evaluate a root goal.
    pub fn evaluate_root_goal(&mut self, goal: Goal) -> SolverResult {
        let canonical_goal =
            canonicalize_goal(goal, self.interner, &mut self.infcx, self.max_universe);
        self.evaluate_canonical_goal(canonical_goal)
    }

    /// Borrow the inference context, e.g. to inspect resolved type variables
    /// after a successful root evaluation.
    pub fn infcx(&self) -> &InferCtxt {
        &self.infcx
    }

    /// Mutable borrow of the inference context, useful for creating fresh
    /// inference variables before passing a goal into the solver.
    pub fn infcx_mut(&mut self) -> &mut InferCtxt {
        &mut self.infcx
    }

    /// The maximum universe index visible to the solver.
    pub fn max_universe(&self) -> UniverseIndex {
        self.max_universe
    }

    // -----------------------------------------------------------------------
    // Core recursive loop
    // -----------------------------------------------------------------------

    /// Evaluate a canonical goal.
    pub fn evaluate_canonical_goal(&mut self, canonical_goal: CanonicalGoal) -> SolverResult {
        // Set up this goal's canonical-variable context. This must happen before
        // cycle handling so that `make_response` can populate `var_values` for
        // the current goal.
        let prev_variables = self.canonical_variables;
        let prev_var_map = std::mem::take(&mut self.canonical_var_map);
        let (instantiated_goal, var_map) = crate::instantiate::instantiate_with_mapping(
            canonical_goal,
            self.interner,
            &mut self.infcx,
        );
        self.canonical_variables = canonical_goal.variables;
        self.canonical_var_map = var_map;

        // 1. Check the cache, respecting the remaining depth budget.
        if let Some(entry) = self
            .search_graph
            .lookup_cache(&canonical_goal, self.available_depth)
        {
            self.canonical_variables = prev_variables;
            self.canonical_var_map = prev_var_map;
            return Ok(entry.result.clone());
        }

        // 2. Check for cycles.
        if let Some(stack_index) = self.search_graph.is_in_stack(&canonical_goal) {
            let result = self.handle_cycle(stack_index, canonical_goal);
            self.canonical_variables = prev_variables;
            self.canonical_var_map = prev_var_map;
            return result;
        }

        // 3. Push onto stack and evaluate.
        self.search_graph.push(canonical_goal, self.available_depth);

        let snapshot = self.infcx.snapshot();
        let mut result = self.compute_goal(instantiated_goal);

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
                let provisional = self.make_response(Certainty::Yes);
                self.search_graph.set_provisional(depth - 1, provisional);
                result = self.compute_goal(instantiated_goal);
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

        // Restore the caller's canonical-variable context.
        self.canonical_variables = prev_variables;
        self.canonical_var_map = prev_var_map;

        result
    }

    /// Handle a goal that is already on the evaluation stack.
    fn handle_cycle(&mut self, stack_index: usize, canonical_goal: CanonicalGoal) -> SolverResult {
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
            let provisional = self.make_response(Certainty::Yes);
            self.search_graph
                .set_provisional(stack_index, provisional.clone());
            Ok(provisional)
        } else {
            Ok(self.make_response(Certainty::Maybe))
        }
    }

    /// True if the given goal may be solved coinductively.
    fn is_coinductive_goal(&self, goal: &Goal) -> bool {
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
    fn compute_goal(&mut self, goal: Goal) -> SolverResult {
        match goal.predicate {
            Predicate::Trait(trait_pred) => self.compute_trait_goal(goal, trait_pred),
            Predicate::Projection(proj_pred) => self.compute_projection_like_goal(
                goal.param_env,
                proj_pred.projection_ty,
                Some(proj_pred.term),
            ),
            Predicate::NormalizesTo(norm_pred) => self.compute_projection_like_goal(
                goal.param_env,
                norm_pred.projection_ty,
                Some(norm_pred.term),
            ),
            Predicate::WellFormed(wf_pred) => {
                // TODO(Phase 5): structural well-formedness.
                if matches!(self.interner.ty(wf_pred.ty), Ty::Error) {
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

    // -----------------------------------------------------------------------
    // Trait goals
    // -----------------------------------------------------------------------

    /// Compute a trait goal.
    fn compute_trait_goal(&mut self, goal: Goal, trait_pred: TraitPredicate) -> SolverResult {
        let candidates = self.assemble_candidates(goal, trait_pred);

        if candidates.is_empty() {
            // An unresolved inference variable as `Self` is genuinely ambiguous:
            // the variable may later be resolved to a type that implements the
            // trait. Return `Maybe` instead of `NoSolution`.
            let self_ty = self
                .trait_self_ty(&trait_pred.trait_ref)
                .map(|ty| self.resolve_ty(ty))
                .unwrap_or(self.interner.mk_ty(Ty::Error));
            if matches!(self.interner.ty(self_ty), Ty::Infer(InferTy::TyVar(_))) {
                return Ok(self.make_response(Certainty::Maybe));
            }
            return Err(NoSolution);
        }

        // Evaluate all candidates in isolation and collect their results.
        let mut yes_candidates: Vec<Candidate> = Vec::new();
        let mut maybe_count = 0;

        for candidate in &candidates {
            let response = self.probe(|this| this.try_candidate(goal, candidate));
            match response {
                Ok(ref r) if r.value.certainty == Certainty::Yes => {
                    yes_candidates.push(candidate.clone());
                }
                Ok(ref r) if r.value.certainty == Certainty::Maybe => {
                    maybe_count += 1;
                }
                _ => {}
            }
        }

        // Decide which candidate to commit. Non-auto-trait candidates (user
        // impls, built-ins, param-env assumptions) take precedence over
        // auto-trait structural derivation. This avoids ambiguity when a type
        // has both a user-written impl and a derivable auto-trait impl.
        let selected = if yes_candidates.is_empty() {
            None
        } else {
            let non_auto: Vec<_> = yes_candidates
                .iter()
                .filter(|c| !matches!(c.source, CandidateSource::AutoTrait))
                .cloned()
                .collect();
            match non_auto.len() {
                0 => {
                    // Only auto-trait candidates succeeded. Commit a unique one.
                    if yes_candidates.len() == 1 {
                        Some(yes_candidates[0].clone())
                    } else {
                        self.tainted = Err(NoSolution);
                        return Ok(self.make_response(Certainty::Maybe));
                    }
                }
                1 => Some(non_auto[0].clone()),
                _ => {
                    // Multiple non-auto-trait `Yes` candidates: overlap.
                    self.tainted = Err(NoSolution);
                    return Ok(self.make_response(Certainty::Maybe));
                }
            }
        };

        match selected {
            Some(candidate) => {
                let response = self.try_candidate(goal, &candidate)?;
                let certainty =
                    self.elaborate_supertraits(goal, trait_pred, response.value.certainty)?;
                Ok(self.make_response(certainty))
            }
            None if maybe_count > 0 => Ok(self.make_response(Certainty::Maybe)),
            None => Err(NoSolution),
        }
    }

    /// Assemble candidates for a trait goal.
    fn assemble_candidates(&mut self, goal: Goal, trait_pred: TraitPredicate) -> Vec<Candidate> {
        let mut candidates = Vec::new();
        let is_positive = trait_pred.polarity == yelang_ty::ty::ImplPolarity::Positive;

        // Param-env assumptions.
        for &pred in goal.param_env.caller_bounds.iter() {
            if let Predicate::Trait(assumption) = pred {
                if assumption.polarity != trait_pred.polarity {
                    continue;
                }
                if assumption.trait_ref.def_id == trait_pred.trait_ref.def_id {
                    candidates.push(Candidate {
                        source: CandidateSource::ParamEnv(pred),
                    });
                }
            }
        }

        if is_positive {
            // Built-in rules (positive goals only).
            if let Some(kind) = self.tcx.builtin_kind(trait_pred.trait_ref.def_id) {
                candidates.push(Candidate {
                    source: CandidateSource::Builtin(kind),
                });
            }

            // User-written impls.
            for impl_info in self.tcx.impls_for_trait(trait_pred.trait_ref.def_id) {
                if impl_info.polarity == yelang_ty::ty::ImplPolarity::Positive {
                    candidates.push(Candidate {
                        source: CandidateSource::UserImpl(impl_info.clone()),
                    });
                }
            }

            // Auto-trait derivation.
            if let Some(info) = self.tcx.trait_info(trait_pred.trait_ref.def_id) {
                if info.is_auto {
                    candidates.push(Candidate {
                        source: CandidateSource::AutoTrait,
                    });
                }
            }
        } else {
            // Negative goals: only negative user impls and negative assumptions.
            for impl_info in self.tcx.impls_for_trait(trait_pred.trait_ref.def_id) {
                if impl_info.polarity == yelang_ty::ty::ImplPolarity::Negative {
                    candidates.push(Candidate {
                        source: CandidateSource::UserImpl(impl_info.clone()),
                    });
                }
            }
        }

        candidates
    }

    /// Elaborate supertraits after a trait goal has been proven.
    ///
    /// If `trait Foo: Bar + Baz` and the goal is `T: Foo`, this also requires
    /// `T: Bar` and `T: Baz` with the same polarity as the original goal.
    fn elaborate_supertraits(
        &mut self,
        goal: Goal,
        trait_pred: TraitPredicate,
        base_certainty: Certainty,
    ) -> Result<Certainty, NoSolution> {
        let trait_info = match self.tcx.trait_info(trait_pred.trait_ref.def_id) {
            Some(info) => info,
            None => return Ok(base_certainty),
        };

        let subst = Substitution::from_args(trait_pred.trait_ref.args.iter().cloned().collect());
        let mut certainty = base_certainty;

        for &super_trait_ref in &trait_info.supertraits {
            let super_trait_ref = substitute(self.interner, super_trait_ref, &subst);
            let super_pred = Predicate::Trait(TraitPredicate {
                trait_ref: super_trait_ref,
                polarity: trait_pred.polarity,
            });
            let response = self.add_goal(Goal::new(goal.param_env, super_pred))?;
            if response.value.certainty == Certainty::Maybe {
                certainty = Certainty::Maybe;
            }
        }

        Ok(certainty)
    }

    /// Evaluate a single candidate. On success this commits the unifications
    /// and nested-goal proofs performed by the candidate.
    fn try_candidate(&mut self, goal: Goal, candidate: &Candidate) -> SolverResult {
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

    fn try_param_env_candidate(&mut self, goal: Goal, assumption: Predicate) -> SolverResult {
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
            .eq_generic_args(
                self.interner,
                &assumption.trait_ref.args,
                &goal_pred.trait_ref.args,
            )
            .map_err(|_| NoSolution)?;

        Ok(self.make_response(Certainty::Yes))
    }

    fn try_builtin_candidate(&mut self, goal: Goal, kind: BuiltinTraitKind) -> SolverResult {
        let Predicate::Trait(trait_pred) = goal.predicate else {
            return Err(NoSolution);
        };
        let self_ty = self.trait_self_ty(&trait_pred.trait_ref)?;

        let satisfied = match kind {
            BuiltinTraitKind::Sized => is_sized(self_ty, self.interner),
            BuiltinTraitKind::Copy => is_copy(self_ty, self.interner),
            BuiltinTraitKind::Clone => is_clone(self_ty, self.interner),
        };

        if satisfied {
            Ok(self.make_response(Certainty::Yes))
        } else {
            Err(NoSolution)
        }
    }

    fn try_user_impl_candidate(&mut self, goal: Goal, impl_info: &ImplInfo) -> SolverResult {
        let goal_pred = match goal.predicate {
            Predicate::Trait(tp) => tp,
            _ => return Err(NoSolution),
        };

        match self.try_impl_substitution(
            goal.param_env,
            goal_pred.trait_ref,
            goal_pred.polarity,
            impl_info,
        )? {
            Some(_) => Ok(self.make_response(Certainty::Yes)),
            None => Ok(self.make_response(Certainty::Maybe)),
        }
    }

    /// Try to match a specific user impl against a trait goal, returning the
    /// impl substitution if the impl applies with certainty `Yes`, or `None`
    /// if it is merely ambiguous (`Maybe`).
    fn try_impl_substitution(
        &mut self,
        param_env: ParamEnv,
        goal_trait_ref: TraitRef,
        goal_polarity: yelang_ty::ty::ImplPolarity,
        impl_info: &ImplInfo,
    ) -> Result<Option<Substitution>, NoSolution> {
        if impl_info.trait_ref.def_id != goal_trait_ref.def_id {
            return Err(NoSolution);
        }
        if impl_info.polarity != goal_polarity {
            return Err(NoSolution);
        }

        // Create fresh inference variables for each impl generic parameter.
        let subst = self.fresh_impl_substitution(impl_info.generic_param_count);

        let impl_trait_ref = substitute(self.interner, impl_info.trait_ref, &subst);
        let impl_predicates: Vec<_> = impl_info
            .predicates
            .iter()
            .map(|&p| substitute(self.interner, p, &subst))
            .collect();

        self.infcx
            .eq_trait_refs(self.interner, &impl_trait_ref, &goal_trait_ref)
            .map_err(|_| NoSolution)?;

        let mut certainty = Certainty::Yes;
        for pred in impl_predicates {
            let response = self.add_goal(Goal::new(param_env, pred))?;
            if response.value.certainty == Certainty::Maybe {
                certainty = Certainty::Maybe;
            }
        }

        match certainty {
            Certainty::Yes => Ok(Some(subst)),
            Certainty::Maybe => Ok(None),
            Certainty::No => unreachable!("impl substitution certainty is only Yes or Maybe"),
        }
    }

    /// Create a fresh substitution for an impl's generic parameters.
    fn fresh_impl_substitution(&mut self, generic_param_count: usize) -> Substitution {
        let mut subst_args = Vec::with_capacity(generic_param_count);
        for _ in 0..generic_param_count {
            subst_args.push(GenericArg::Type(self.infcx.new_ty_var(self.interner)));
        }
        Substitution::from_args(subst_args)
    }

    fn try_auto_trait_candidate(&mut self, goal: Goal) -> SolverResult {
        let trait_pred = match goal.predicate {
            Predicate::Trait(tp) => tp,
            _ => return Err(NoSolution),
        };

        let self_ty = self.trait_self_ty(&trait_pred.trait_ref)?;
        let self_ty = self.resolve_ty(self_ty);

        // Collect all component types that must satisfy the auto trait.
        let mut component_tys: Vec<TyId> = Vec::new();

        match self.interner.ty(self_ty) {
            // Primitives, functions, and never are always auto-trait-safe.
            Ty::Bool
            | Ty::Char
            | Ty::Int(_)
            | Ty::Uint(_)
            | Ty::Float(_)
            | Ty::Str
            | Ty::Never
            | Ty::FnPtr(_)
            | Ty::FnDef(_)
            | Ty::TypeLit(_)
            | Ty::Utility(_, _)
            | Ty::Error => {}

            // ADTs: require the trait for every field (with the ADT's generic
            // arguments substituted) and every type argument.
            Ty::Adt(adt, args) => {
                let subst = self.adt_substitution(&args);
                for field_ty in self.tcx.adt_field_tys(adt.def_id) {
                    component_tys.push(substitute(self.interner, *field_ty, &subst));
                }
                for arg in args.iter() {
                    if let GenericArg::Type(ty) = arg {
                        component_tys.push(*ty);
                    }
                }
            }

            // Tuples: require the trait for every element.
            Ty::Tuple(args) => {
                for arg in args.iter() {
                    if let GenericArg::Type(ty) = arg {
                        component_tys.push(*ty);
                    }
                }
            }

            // Arrays and slices: require the trait for the element type.
            Ty::Array(ty, _) | Ty::Slice(ty) => {
                component_tys.push(ty);
            }

            // References and raw pointers: require the trait for the pointee.
            Ty::Ref(ty, _) | Ty::RawPtr(yelang_ty::ty::TypeAndMut { ty, .. }) => {
                component_tys.push(ty);
            }

            // Anonymous structs: require the trait for every field.
            Ty::AnonStruct(anon) => {
                for field in anon.fields.iter() {
                    component_tys.push(field.ty);
                }
            }

            // Unions: require the trait for both alternatives.
            Ty::Union(a, b) => {
                component_tys.push(a);
                component_tys.push(b);
            }

            // Projections: try to normalize and derive on the normalized type.
            Ty::Projection(projection_ty) => {
                if let Some(ty) = self.normalize_projection_ty(goal.param_env, projection_ty) {
                    component_tys.push(ty);
                } else {
                    return Ok(self.make_response(Certainty::Maybe));
                }
            }

            // Aliases: conservatively ambiguous until alias expansion is implemented.
            Ty::Alias(_) => {
                return Ok(self.make_response(Certainty::Maybe));
            }

            // Types that cannot be resolved yet.
            Ty::Infer(_) | Ty::Param(_) | Ty::Placeholder(_) | Ty::Bound(_, _) => {
                return Ok(self.make_response(Certainty::Maybe));
            }

            // Trait objects are conservatively ambiguous.
            Ty::Dynamic(_) => {
                return Ok(self.make_response(Certainty::Maybe));
            }
        }

        let mut certainty = Certainty::Yes;
        for ty in component_tys {
            let nested_pred = Predicate::Trait(TraitPredicate {
                trait_ref: TraitRef {
                    def_id: trait_pred.trait_ref.def_id,
                    args: self.interner.mk_generic_args(&[GenericArg::Type(ty)]),
                },
                polarity: yelang_ty::ty::ImplPolarity::Positive,
            });
            let response = self.add_goal(Goal::new(goal.param_env, nested_pred))?;
            if response.value.certainty == Certainty::Maybe {
                certainty = Certainty::Maybe;
            }
        }

        Ok(self.make_response(certainty))
    }

    /// Extract the `Self` type from a trait reference.
    ///
    /// In Yelang the first generic argument of a `TraitRef` is always `Self`.
    fn trait_self_ty(&self, trait_ref: &TraitRef) -> Result<TyId, NoSolution> {
        trait_ref
            .args
            .iter()
            .next()
            .and_then(|arg| arg.expect_type_checked())
            .ok_or(NoSolution)
    }

    // -----------------------------------------------------------------------
    // Projection normalization
    // -----------------------------------------------------------------------

    /// Compute a `Projection` or `NormalizesTo` goal.
    ///
    /// The goal is to find the unique applicable impl for the projection's
    /// trait ref, compute the substituted associated type, and unify it with
    /// the expected type (if any).
    fn compute_projection_like_goal(
        &mut self,
        param_env: ParamEnv,
        projection_ty: ProjectionTy,
        expected: Option<TyId>,
    ) -> SolverResult {
        let candidates = self.assemble_projection_candidates(param_env, projection_ty);

        let mut yes_results: Vec<(Candidate, TyId)> = Vec::new();
        let mut maybe = false;

        for candidate in candidates {
            let probe_result: Result<Option<TyId>, NoSolution> = self.probe(|this| {
                let ty =
                    match this.try_projection_candidate(param_env, projection_ty, &candidate)? {
                        Some(ty) => ty,
                        None => return Ok(None),
                    };
                if let Some(expected) = expected {
                    this.infcx
                        .eq(this.interner, ty, expected)
                        .map_err(|_| NoSolution)?;
                }
                Ok(Some(ty))
            });

            match probe_result {
                Ok(Some(ty)) => yes_results.push((candidate, ty)),
                Ok(None) => maybe = true,
                Err(_) => {}
            }
        }

        if yes_results.is_empty() {
            return if maybe {
                Ok(self.make_response(Certainty::Maybe))
            } else {
                Err(NoSolution)
            };
        }

        // All `Yes` candidates must agree on the normalized type. If they do,
        // commit the first one; otherwise the goal is ambiguous.
        let first_ty = yes_results[0].1;
        let all_agree = yes_results.iter().all(|(_, ty)| *ty == first_ty);

        if !all_agree {
            self.tainted = Err(NoSolution);
            return Ok(self.make_response(Certainty::Maybe));
        }

        if maybe {
            // A `Yes` candidate exists but another candidate is merely ambiguous.
            return Ok(self.make_response(Certainty::Maybe));
        }

        // Commit the unique agreeing candidate.
        let selected = &yes_results[0].0;
        if let Some(ty) = self.try_projection_candidate(param_env, projection_ty, selected)? {
            if let Some(expected) = expected {
                self.infcx
                    .eq(self.interner, ty, expected)
                    .map_err(|_| NoSolution)?;
            }
            Ok(self.make_response(Certainty::Yes))
        } else {
            // The committed candidate became ambiguous; this should not happen
            // in a well-behaved probe but we handle it conservatively.
            Ok(self.make_response(Certainty::Maybe))
        }
    }

    /// Assemble candidates that could potentially normalize a projection type.
    fn assemble_projection_candidates(
        &mut self,
        param_env: ParamEnv,
        projection_ty: ProjectionTy,
    ) -> Vec<Candidate> {
        let trait_pred = TraitPredicate {
            trait_ref: projection_ty.trait_ref,
            polarity: yelang_ty::ty::ImplPolarity::Positive,
        };
        let goal = Goal::new(param_env, Predicate::Trait(trait_pred));
        self.assemble_candidates(goal, trait_pred)
            .into_iter()
            .filter(|c| matches!(c.source, CandidateSource::UserImpl(_)))
            .collect()
    }

    /// Try a single projection-normalization candidate. Returns the normalized
    /// type on `Yes`, `None` on ambiguity (`Maybe`), and `NoSolution` on failure.
    fn try_projection_candidate(
        &mut self,
        param_env: ParamEnv,
        projection_ty: ProjectionTy,
        candidate: &Candidate,
    ) -> Result<Option<TyId>, NoSolution> {
        match &candidate.source {
            CandidateSource::UserImpl(impl_info) => {
                match self.try_impl_substitution(
                    param_env,
                    projection_ty.trait_ref,
                    yelang_ty::ty::ImplPolarity::Positive,
                    impl_info,
                )? {
                    Some(subst) => {
                        let resolved_subst = self.resolve_substitution(&subst);
                        match crate::normalize::assoc_type_from_impl(
                            self.tcx,
                            self.interner,
                            projection_ty,
                            impl_info.def_id,
                            &resolved_subst,
                        ) {
                            Some(ty) => Ok(Some(self.resolve_ty(ty))),
                            None => Err(NoSolution),
                        }
                    }
                    None => Ok(None),
                }
            }
            _ => Err(NoSolution),
        }
    }

    /// Try to normalize a projection type without an expected term.
    ///
    /// Used by auto-trait derivation on projection types. Returns `Some` only
    /// when a unique, non-ambiguous impl is available.
    fn normalize_projection_ty(
        &mut self,
        param_env: ParamEnv,
        projection_ty: ProjectionTy,
    ) -> Option<TyId> {
        let candidates = self.assemble_projection_candidates(param_env, projection_ty);
        let mut yes_results: Vec<TyId> = Vec::new();
        let mut maybe = false;

        for candidate in candidates {
            match self
                .probe(|this| this.try_projection_candidate(param_env, projection_ty, &candidate))
            {
                Ok(Some(ty)) => yes_results.push(ty),
                Ok(None) => maybe = true,
                Err(_) => {}
            }
        }

        if maybe || yes_results.is_empty() {
            return None;
        }

        let first = yes_results[0];
        if yes_results.iter().all(|ty| *ty == first) {
            Some(first)
        } else {
            None
        }
    }

    // -----------------------------------------------------------------------
    // Substitution and variable resolution helpers
    // -----------------------------------------------------------------------

    /// Build a substitution that maps an ADT's type parameters to the concrete
    /// arguments used in a particular `Adt` type.
    fn adt_substitution(&self, args: &yelang_ty::list::List<GenericArg>) -> Substitution {
        Substitution::from_args(args.iter().cloned().collect())
    }

    /// Resolve inference variables inside a substitution as far as possible.
    fn resolve_substitution(&mut self, subst: &Substitution) -> Substitution {
        let args: Vec<_> = subst
            .args
            .iter()
            .map(|&arg| match arg {
                GenericArg::Type(ty) => GenericArg::Type(self.resolve_ty(ty)),
                GenericArg::Const(ct) => GenericArg::Const(ct),
            })
            .collect();
        Substitution::from_args(args)
    }

    /// Recursively resolve general type variables.
    fn resolve_ty(&mut self, ty: TyId) -> TyId {
        let mut current = ty;
        loop {
            match self.interner.ty(current) {
                Ty::Infer(InferTy::TyVar(vid)) => {
                    let value = self.infcx.probe_ty_var(vid).clone();
                    match value {
                        TypeVarValue::Known(known) => current = known,
                        TypeVarValue::Unknown => return current,
                    }
                }
                _ => return current,
            }
        }
    }

    // -----------------------------------------------------------------------
    // Nested goals and probes
    // -----------------------------------------------------------------------

    /// Add a nested goal, consuming one level of recursion depth.
    fn add_goal(&mut self, goal: Goal) -> SolverResult {
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
    fn make_response(&mut self, certainty: Certainty) -> CanonicalResponse {
        let var_values: Vec<CanonicalVarValue> = self
            .canonical_variables
            .iter()
            .enumerate()
            .map(|(index, _)| {
                let mapping = self.canonical_var_map.get(index).copied();
                match mapping {
                    Some(CanonicalVarMapping::Ty(vid)) => {
                        let root = self.infcx.find_ty_var(vid);
                        match self.infcx.probe_ty_var(root) {
                            TypeVarValue::Known(ty) => CanonicalVarValue::Ty(*ty),
                            TypeVarValue::Unknown => CanonicalVarValue::Unknown,
                        }
                    }
                    Some(CanonicalVarMapping::Int(vid)) => {
                        let root = self.infcx.find_int_var(vid);
                        match self.infcx.probe_int_var(root) {
                            IntVarValue::Known(it) => CanonicalVarValue::Int(*it),
                            IntVarValue::Unknown => CanonicalVarValue::Unknown,
                        }
                    }
                    Some(CanonicalVarMapping::Float(vid)) => {
                        let root = self.infcx.find_float_var(vid);
                        match self.infcx.probe_float_var(root) {
                            FloatVarValue::Known(ft) => CanonicalVarValue::Float(*ft),
                            FloatVarValue::Unknown => CanonicalVarValue::Unknown,
                        }
                    }
                    Some(CanonicalVarMapping::Const(vid)) => {
                        let root = self.infcx.find_const_var(vid);
                        match self.infcx.probe_const_var(root) {
                            ConstVarValue::Known(ct) => CanonicalVarValue::Const(*ct),
                            ConstVarValue::Unknown => CanonicalVarValue::Unknown,
                        }
                    }
                    Some(CanonicalVarMapping::Placeholder(_)) | None => CanonicalVarValue::Unknown,
                }
            })
            .collect();

        Canonical::new(
            Response {
                certainty,
                goals: List::empty(),
                var_values: self.interner.mk_canonical_var_values(&var_values),
            },
            self.max_universe,
            self.canonical_variables,
        )
    }
}

// -----------------------------------------------------------------------------
// Helper extension
// -----------------------------------------------------------------------------

trait ExpectTypeExt {
    fn expect_type_checked(self) -> Option<TyId>;
}

impl ExpectTypeExt for GenericArg {
    fn expect_type_checked(self) -> Option<TyId> {
        match self {
            GenericArg::Type(ty) => Some(ty),
            GenericArg::Const(_) => None,
        }
    }
}
