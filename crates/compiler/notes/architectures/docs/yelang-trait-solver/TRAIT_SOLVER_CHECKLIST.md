# Trait Solver Production-Readiness Checklist

Legend:
- **Impl**: code is implemented.
- **Test**: there is a dedicated, passing test.
- **Doc**: behaviour is documented in the phase design doc or module docs.

## Phase 3 — Canonicalization and instantiation

- [x] `Canonicalizer` replaces free inference vars with bound vars.
- [x] `Canonicalizer` shifts existing bound variables out by one binder.
- [x] `Canonicalizer` preserves placeholders with their universe.
- [x] `InstantiationCtxt` creates fresh inference vars / placeholders.
- [x] `instantiate` shifts bound variables back in.
- [x] `CanonicalGoal = Canonical<Goal>` includes param-env.
- [x] Tests: `tests/canonicalize.rs`.

## Phase 4 — Recursive solver core

### Data model

- [x] `SolverCtxt` trait decouples solver from `TyCtxt`.
- [x] `TraitDefInfo` exposes `is_auto`, `supertraits`.
- [x] `ImplInfo` exposes `trait_ref`, `generic_param_count`, `predicates`.
- [x] `BuiltinTraitKind` enum (`Sized`, `Copy`, `Clone`).
- [x] `Candidate` / `CandidateSource` cover all candidate kinds.
- [x] `SearchGraph` tracks stack entries, cycle flags, and provisional results.
- [x] `SearchGraph` global cache stores `available_depth` per entry.

### Solver loop

- [x] `EvalCtxt::evaluate_root_goal` canonicalizes and dispatches.
- [x] `EvalCtxt::evaluate_canonical_goal` checks cache + stack.
- [x] Cache lookup respects remaining `available_depth`.
- [x] Depth limit returns `Maybe` (overflow) without panicking.
- [x] Cycle detection returns provisional `Yes` for coinductive goals.
- [x] Inductive cycles return `Maybe`.
- [x] Coinductive cycle fixpoint iterates until stable.
- [x] Only cycle roots are cached; participants are not cached independently.
- [x] `EvalCtxt::probe` isolates candidate evaluation via `InferCtxt::probe`.

### Candidate assembly

- [x] Param-env candidate filters by trait def-id and polarity.
- [x] Param-env candidate unifies `TraitRef` args during evaluation.
- [x] User-impl candidate creates fresh inference vars for impl generics.
- [x] User-impl candidate substitutes and unifies self + trait args.
- [x] User-impl candidate recurses on impl where-clauses.
- [x] Built-in `Sized` candidate.
- [x] Built-in `Copy` candidate.
- [x] Built-in `Clone` candidate.
- [x] `AutoTrait` candidate source with structural derivation.
- [x] `Blanket` candidate source exists (handled by generic user impls).

### Predicate dispatch

- [x] `Predicate::Trait` uses full candidate assembly.
- [x] `Predicate::Projection` resolves via projection normalization.
- [x] `Predicate::NormalizesTo` resolves via projection normalization.
- [x] `Predicate::WellFormed` returns `Yes` for non-error types.
- [x] `Predicate::TypeOutlives` returns `Yes`.
- [x] `Predicate::ConstEvaluatable` returns `Yes` (stub for const eval).

### Tests

- [x] Param-env success / failure.
- [x] Simple and generic user impls.
- [x] Generic impl with failing where-clause.
- [x] Built-in `Sized` / `Copy` / `Clone`.
- [x] Inductive cycle → overflow.
- [x] Coinductive auto-trait cycle → success.
- [x] Auto-trait derivation for ADTs, tuples, references.
- [x] Depth-limit overflow.
- [x] Cache reuse.
- [x] Overlap ambiguity.
- [x] No impl → `NoSolution`.
- [x] Projection normalization (simple, generic, failure, equality).
- [x] Negative impls / polarity.
- [x] Supertrait elaboration.
- [x] Blanket impls.

### Documentation

- [x] `PHASE4_SOLVER_CORE_DESIGN.md` written and up to date.
- [x] Module-level docs explain cycle handling and caching.

## Phase 5 — Candidate assembly and projection normalization

- [x] Implement associated-type normalization.
- [x] Implement `Projection` / `NormalizesTo` predicate dispatch.
- [x] `impl_assoc_items` matched to trait items by ident for robust lookup.
- [x] Auto-trait derivation from ADT fields.
- [x] Blanket impl evaluation.
- [x] Negative impls / negative polarity handling.
- [x] Supertrait elaboration.
- [x] Ambiguity stalling and deferred nested goals.

## Phase 6 — Integration with body type checker

- [x] `impl SolverCtxt for TyCtxt` in `yelang-tycheck/src/solver_ctxt.rs`.
- [x] Collector param indices correct for generic impls and `Self`.
- [x] Collector impl-block `ImplDefId` assignment correct.
- [x] Lower trait generic arguments into `TraitRef::args`.
- [x] `FnCtxt` creates `EvalCtxt` and proves trait goals.
- [x] Method dispatch uses solver for trait selection.
- [x] Method dispatch uses solver for `Deref` projection normalization.
- [x] Solver responses applied back to body `InferCtxt`.

## Phase 7 — Production hardening (remaining work)

- [ ] Universe constraints on existential variables.
- [ ] User-facing diagnostics for `NoSolution` and ambiguous goals.
- [ ] Negative trait bounds in user-written code.
- [ ] Const generics evaluation.
- [ ] Caching strategy review for incremental compilation.
