# Phase 4 — Recursive Next-Generation Trait Solver Core

This document is the authoritative design for the `yelang-trait-solver` core.
It is written for a greenfield implementation and intentionally avoids every
legacy design that rustc / other languages have already shown to be suboptimal.
The solver follows the *recursive goal-driven* architecture used by
`rustc_next_trait_solver` and Chalk, not the old `select`/`fulfill` pipeline.

## Sources and prior art

- [Caching — Rust Compiler Development Guide](https://rustc-dev-guide.rust-lang.org/solve/caching.html)
- [Coinduction — Rust Compiler Development Guide](https://rustc-dev-guide.rust-lang.org/solve/coinduction.html)
- [The search graph and caching — Chalk book](https://rust-lang.github.io/chalk/book/recursive/search_graph.html)

## Goals of this phase

1. Evaluate goals recursively: `Goal { param_env, predicate } → CanonicalResponse`.
2. Support the full predicateKinds that already exist in `yelang-ty::predicate`.
3. Assemble candidates from:
   - param-env assumptions,
   - user-written impl blocks,
   - built-in rules (`Sized`, `Copy`, `Clone`),
   - (stub) auto-trait derivation and blanket impls.
4. Detect cycles and handle them soundly:
   - coinductive cycles iterate to a `Yes` fixpoint,
   - inductive cycles return `Maybe` (overflow/ambiguous),
   - depth-limited recursion returns `Maybe`.
5. Cache results in a global `SearchGraph` keyed by the *whole* canonical goal
   (`param_env + predicate`), respecting the remaining available depth.
6. Provide a `SolverCtxt` trait so the solver crate stays independent of
   `yelang-tycheck` / `yelang-hir` (no dependency cycle).
7. Leave no hand-wavy TODOs in the loop itself: the core must be testable and
   robust even if some predicate kinds still delegate to stubs.

## What this phase does NOT do

- Full associated-type normalization (Phase 5).
- Auto-trait derivation from ADT fields (Phase 5).
- Integration with `FnCtxt` / body type checking (Phase 6).
- `impl SolverCtxt for TyCtxt` (Phase 6; collector fixes required first).

These are explicitly listed in `TRAIT_SOLVER_CHECKLIST.md` so they cannot be
forgotten.

## File tree

```
yelang-trait-solver/
├── Cargo.toml
├── PHASE4_SOLVER_CORE_DESIGN.md      <- this file
├── TRAIT_SOLVER_CHECKLIST.md         <- phase-by-phase checklist
└── src/
    ├── lib.rs                        <- module re-exports
    ├── solver_ctx.rs                 <- SolverCtxt trait + TraitDefInfo + ImplInfo
    ├── search_graph.rs               <- stack, global cache, cycle tracking
    ├── eval_ctxt.rs                  <- EvalCtxt: the recursive engine
    ├── candidate.rs                  <- Candidate / CandidateSource
    ├── builtin.rs                    <- conservative built-in trait rules
    ├── canonicalize.rs               <- Goal → CanonicalGoal
    ├── instantiate.rs                <- CanonicalGoal → fresh inference vars
    ├── goal.rs                       <- Goal struct
    ├── response.rs                   <- CanonicalResponse, NestedGoal, GoalSource
    └── normalize.rs                  <- projection normalization stub (Phase 5)
    └── tests/
        ├── mod.rs
        ├── canonicalize.rs           <- existing canonicalization tests
        ├── builtin.rs                <- existing built-in rule tests
        └── solver.rs                 <- new exhaustive solver tests
```

## Core data structures

### `Goal<'tcx>`

```rust
pub struct Goal<'tcx> {
    pub param_env: ParamEnv<'tcx>,   // caller_bounds: List<Predicate>
    pub predicate: Predicate<'tcx>,
}
```

The whole goal (param-env + predicate) is canonicalized. Two goals that differ
only by the name of an inference variable share the same cache entry; two goals
with different param-envs do **not**.

### `CanonicalResponse<'tcx>`

```rust
pub type CanonicalResponse<'tcx> = Canonical<'tcx, Response<'tcx>>;

pub struct Response<'tcx> {
    pub certainty: Certainty,        // Yes | Maybe | No
    pub goals: List<Canonical<'tcx, Predicate<'tcx>>>,
}
```

For Phase 4 the solver evaluates nested goals eagerly, so responses produced by
`EvalCtxt` carry an empty `goals` list. The list is kept in the IR because
Phase 5/6 will move to lazy nested-goal emission.

### `SearchGraph<'tcx>`

The search graph is **both** the DFS stack *and* the global cache. It has three
jobs:

1. Detect cycles (`stack` lookup).
2. Store *interim* results while a cycle is being resolved (`provisional`).
3. Store *final* results after a goal is fully evaluated (`cache`).

```rust
pub struct SearchGraph<'tcx> {
    stack: Vec<StackEntry<'tcx>>,
    cache: FxHashMap<CanonicalGoal<'tcx>, CacheEntry<'tcx>>,
}

pub struct StackEntry<'tcx> {
    pub goal: CanonicalGoal<'tcx>,
    pub available_depth: usize,
    pub coinductive: bool,
    pub provisional: Option<CanonicalResponse<'tcx>>,
    pub has_cycle: bool,
}

pub struct CacheEntry<'tcx> {
    pub result: CanonicalResponse<'tcx>,
    pub available_depth: usize,   // result proven with at least this much budget left
}
```

Cache lookup is sound only when the current `available_depth` is **≥** the entry’s
`available_depth`. A result proven with a larger remaining budget can be reused
with a smaller budget, but not vice-versa.

## Solver loop

```text
Goal
  │
  ▼
canonicalize(goal) ──► CanonicalGoal
  │
  ▼
SearchGraph::lookup_cache (respect available_depth)
  │
  ▼  cache miss
SearchGraph::is_in_stack?
  │
  ├─ yes + coinductive ──► return provisional Yes, mark cycle
  ├─ yes + inductive ────► return Maybe (overflow)
  └─ no ─────────────────► push
                              │
                              ▼
                      compute_goal (dispatch on predicate kind)
                              │
                              ▼
                      for trait goals: assemble candidates
                              │
                              ▼
                      evaluate each candidate in an isolated probe
                              │
                              ▼
                      merge / select unique Yes
                              │
                              ▼
                      pop + cache (only non-cycle-participants)
```

## Depth limit and overflow

`EvalCtxt` carries an `available_depth` counter. Every recursive call consumes
one unit. When `available_depth == 0`, the solver returns `Certainty::Maybe`
instead of panicking or erroring. This is the same semantics as rustc’s new
solver: overflow is ambiguity, not a hard error.

The default root budget is large enough for realistic programs but small enough
to make pathological inputs terminate quickly. Tests explicitly exercise the
limit.

## Cycle handling

A cycle is detected when the canonical goal is already on the `SearchGraph`
stack.

- **Coinductive goal** (auto traits, `Sized`, well-formedness): the cycle is
  allowed to represent an infinite proof tree. The first time the cycle is hit,
  the solver returns a provisional `Yes`. After the head of the cycle finishes
  its first pass, the solver compares the computed result with the provisional
  result. If they match, the cycle is resolved; otherwise it iterates up to a
  small bound. This is the standard fixpoint algorithm from Chalk / rustc.

- **Inductive goal** (ordinary user traits): a cycle means there is no finite
  proof tree, so the result is `Maybe` (overflow/ambiguous). We do **not** cache
  inductive cycle results.

- Only the *root* of a cycle is cached. Participants stay in the search graph
  only while the root is being resolved. This matches the rustc guidance that
  cycle participants must not be moved to the global cache independently.

## Candidate evaluation and probes

Candidates must be evaluated in isolation. `InferCtxt::probe` snapshots the
unification state, runs the candidate, and rolls back. This prevents a failed
candidate from leaving stale variable assignments.

After all candidates have been probed:

- exactly one `Yes` candidate → select it, re-run it **outside** a probe to
  commit its unifications, and recursively prove its nested goals;
- zero `Yes`, at least one `Maybe` → `Maybe`;
- more than one `Yes` → `Maybe` (ambiguous overlap);
- zero candidates → `NoSolution`.

## `_` (underscore) as an inference wildcard

Yelang treats `_` as a request for inference everywhere a type is expected,
including return-type position. This is stronger than Rust, where `_` is not
permitted in item signatures. Because the solver is goal-driven, a return type
written as `_` simply becomes a fresh type variable; the body-checker will emit
goals that the solver resolves. There is no special "return-type inference"
mode—the same canonicalization + recursive evaluation handles it.

## Predicate dispatch

`compute_goal` matches on `Predicate`:

| Predicate kind | Phase 4 behaviour |
|----------------|-------------------|
| `Trait`        | Full candidate assembly (param-env, impls, built-ins). |
| `Projection`   | Equate the projection type with the expected term after normalization; currently a stub returning `Yes`. |
| `NormalizesTo` | Normalize `<T as Trait>::Assoc` and equate; currently a stub returning `Yes`. |
| `WellFormed`   | Structural well-formedness; currently returns `Yes` for non-error types. |
| `TypeOutlives` | No-op (Yelang has no lifetimes). |
| `ConstEvaluatable` | No-op (const eval is Phase 8). |

## Testing strategy

Every semantic case has a dedicated test in `src/tests/solver.rs`:

- param-env success and failure,
- simple user impl,
- generic impl with and without where-clause obligations,
- impl that requires nested trait goals,
- built-in `Sized` and `Copy`,
- inductive cycle → `Maybe`/overflow,
- coinductive auto-trait cycle → `Yes`,
- depth-limit overflow,
- cache reuse,
- overlap/ambiguity (`Maybe`),
- no impl → `NoSolution`.

Tests use a `TestCtxt` implementing `SolverCtxt` so they do not depend on the
still-incomplete HIR collector.

## Checklist integration

This design is paired with `TRAIT_SOLVER_CHECKLIST.md`. Every item in the
"Phase 4" section must be checked before moving to Phase 5. Items that are
explicitly out of scope for Phase 4 are listed under "Phase 5" or "Phase 6"
so they cannot be silently dropped.
