# Trait Solver Checklist

This checklist tracks the implementation status of Yelang's next-generation,
recursive, canonicalizing trait solver (`yelang-trait-solver`).

## Type IR prerequisites (in `yelang-ty`)

- [x] `Ty::Projection` exists and carries `ProjectionTy`.
- [x] `Ty::Dynamic` holds `Binder<List<ExistentialPredicate>>`.
- [x] `Predicate::NormalizesTo` exists.
- [x] `Predicate::WellFormed` exists.
- [x] `Const::Param` exists.
- [x] `TypeFoldable` implemented for `TyId`, `ConstId`, `GenericArg`, `Predicate`,
      `List<Predicate>`, `ExistentialPredicate`, `TraitRef`, `ProjectionTy`.
- [x] `TypeVisitable` implemented for `Predicate` and `List<Predicate>`.
- [x] Interner supports `List<Predicate>` via `mk_predicates`.

## Canonicalization and instantiation

- [x] Canonicalizer replaces free `TyVar`/`IntVar`/`FloatVar`/`ConstVar` with
      bound variables and records their `CanonicalVarKind`.
- [x] Canonicalizer replaces placeholders with placeholder canonical variables
      preserving universe.
- [x] Canonicalizer shifts existing bound variables out by one binder level.
- [x] Canonicalizer resolves known inference variables before binding.
- [x] Instantiation creates fresh inference variables for each canonical var.
- [x] Instantiation creates fresh placeholders for placeholder canonical vars.
- [x] Instantiation shifts bound variables back in by one binder level.
- [x] Round-trip tests for types, consts, predicates, and goals.
- [ ] Handle universe constraints on existential variables (currently ignored).

## Goal representation

- [x] `Goal` contains `ParamEnv` and `Predicate`.
- [x] `Goal` implements `TypeFoldable` and `Hash`.
- [x] `CanonicalGoal = Canonical<Goal>` (not just `Canonical<Predicate>`).

## Search graph

- [x] Separate evaluation stack and global cache.
- [x] Stack detects cycles.
- [x] Cache is keyed by `CanonicalGoal`.
- [x] Cache respects remaining depth via depth-budgeted lookup.
- [x] Track cycle participants for cycle-root correctness.
- [x] Coinductive cycles iterate to fixpoint.
- [x] Inductive cycles return `Maybe` (overflow).
- [x] Overflow returns `Maybe` with depth tracking.

## Solver core (`EvalCtxt`)

- [x] `EvalCtxt` owns an `InferCtxt` and the interner.
- [x] `evaluate_root_goal` canonicalizes before solving.
- [x] Goal dispatch on all `Predicate` variants.
- [x] Candidate evaluation inside isolated `InferCtxt` probes.
- [x] Nested goals proved via `add_goal` with depth tracking.

## Candidate assembly

- [x] Param-env candidates (positive and negative polarity).
- [x] User impl candidates from `TyCtxt` impl index.
- [x] Built-in candidates (`Sized`, `Copy`, `Clone`).
- [x] Auto-trait candidates with coinductive semantics.
- [x] Blanket impl candidates (handled by generic user impls).

## Projection normalization

- [x] `<T as Trait>::Assoc == U` as `Projection` predicate.
- [x] `<T as Trait>::Assoc normalizes-to U` as `NormalizesTo` predicate.
- [x] Find defining impl and substitute associated type.
- [x] Match impl associated items to trait items by name for robust lookup.
- [x] Handle ambiguous normalization (no applicable impl, multiple impls).

## Integration with type checker

- [x] Type checker emits trait obligations (Phase C).
- [x] Solver results are instantiated back into the type checker's `InferCtxt` (Phase D).
- [x] Method dispatch uses the solver for trait selection.
- [x] Method dispatch uses the solver for `Deref` projection normalization.
- [ ] Diagnostics for `NoSolution` and ambiguity (Phase E).

## Testing

- [x] Unit tests for canonicalization and instantiation.
- [x] Built-in `Sized`/`Copy` tests.
- [x] Solver tests for simple trait impls.
- [x] Solver tests for generic impls with where clauses.
- [x] Solver tests for param-env assumptions.
- [x] Solver tests for projection normalization (simple, generic, failure, equality).
- [x] Coinductive auto-trait cycle tests.
- [x] Auto-trait derivation tests (ADT, tuple, reference).
- [x] Negative impl / polarity tests.
- [x] Supertrait elaboration tests.
- [x] Blanket impl tests.
- [x] Ambiguous nested goal / stalling tests.
- [x] Inductive cycle failure / overflow tests.
- [x] Overflow/depth-limit tests.
