# Phase D/E — Method Dispatch and `Deref` Autoderef Design

This document describes the production-ready method-lookup engine in
`yelang-tycheck`.  It covers the rustc-style probe/confirm model, built-in
autoderef, user-defined `Deref` autoderef via projection normalization, and the
receiver-adjustment side table used by later phases ( MIR, borrow checking,
IDE assistance, and incremental compilation).

## 1. Scope and goals

- Resolve `receiver.method(args...)` and `receiver.field` (field dispatch is in
  progress; the same autoderef machinery is reused).
- Prefer inherent candidates over trait (extension) candidates.
- Prefer earlier deref steps over later ones.
- Support built-in autoderef: references and raw pointers.
- Support user-defined `Deref` autoderef through the next-generation trait
  solver using `<T as Deref>::Target normalizes-to U` goals.
- Record the exact receiver adjustments so downstream passes can generate the
  correct code and diagnostics.
- Emit all trait/normalization obligations implied by the chosen dispatch path.

## 2. File tree

```text
yelang-tycheck/src/
├── check.rs              # expression/type statement checking, calls method.rs
├── coerce.rs             # coercion engine (exact match today; deref coercion TODO)
├── collector.rs          # lowers HIR items into TyCtxt tables
├── fn_ctxt.rs            # per-function inference context, solver response writeback
├── hir_ty_lower.rs       # lowers HIR type nodes to interned TyId
├── lower_ctx.rs          # trait for type lowering contexts
├── method.rs             # **method lookup: probe, confirm, autoderef, adjustments**
├── pat.rs                # pattern type checking
├── solver_ctxt.rs        # TyCtxt as SolverCtxt for the trait solver
├── tcx.rs                # global type context, Deref lang items, item tables
├── typeck_results.rs     # per-body results including expr_adjustments
├── writeback.rs          # resolves inference variables after checking
└── tests/mod.rs          # exhaustive tests

yelang-trait-solver/src/
├── builtin.rs            # built-in Sized/Copy/Clone rules
├── candidate.rs          # candidate assembly for trait/projection goals
├── canonicalize.rs       # canonicalization of goals for the cache
├── eval_ctxt.rs          # recursive goal-driven solver engine
├── goal.rs               # Goal = param_env + predicate
├── instantiate.rs        # instantiate canonical goals with fresh infer vars
├── normalize.rs          # associated-type normalization from selected impls
├── response.rs           # responses, certainty, goal sources
├── search_graph.rs       # DFS stack + global cache
├── solver_ctx.rs         # trait the type checker implements
└── tests/                # solver unit tests
```

## 3. Core data structures

### 3.1 `Adjustment`

```rust
pub enum Adjustment {
    Deref,
    Ref,
    RefMut,
    DerefTrait { source: TyId, target: TyId },
}
```

- `Deref` — built-in `*` through a reference or raw pointer.
- `Ref` / `RefMut` — autoref applied at the final probe step.
- `DerefTrait { source, target }` — one user `Deref` step.  `source` is the
  type before the deref, `target` is the normalized `<source as Deref>::Target`.

### 3.2 `CandidateSource`

```rust
pub enum CandidateSource {
    Inherent { impl_id: ImplDefId, item_def_id: DefId },
    Trait { trait_def_id: DefId, item_def_id: DefId, trait_ref: TraitRef },
}
```

### 3.3 `MethodPick`

```rust
pub struct MethodPick {
    pub candidate: MethodCandidate,
    pub receiver_adjustments: Vec<Adjustment>,
    pub probe_ty: TyId,
}
```

`probe_ty` is the receiver type at which the method signature matched.

### 3.4 `TypeckResults::expr_adjustments`

A new side table maps a method-call receiver `ExprId` to the list of
adjustments the compiler must apply.  This is consumed by:

- MIR lowering (insert `Deref` / `DerefMut` / `Borrow` operations).
- Borrow checking (autoref mutability, move/borrow semantics).
- Diagnostics (show the user why `wrapper.foo()` resolved to `Inner::foo`).
- Incremental compilation (adjustments are part of the per-body result).

### 3.5 `TyCtxt` Deref lang items

```rust
pub struct TyCtxt {
    // ... other tables ...
    pub deref_trait: Option<DefId>,   // the `Deref` trait
    pub deref_target: Option<DefId>,  // the `Target` associated type
}
```

`register_deref_lang_item(trait, target)` is called by the driver once the
prelude / lang items are known.

## 4. Algorithm

### 4.1 Probing

`probe_types(fcx, receiver_ty)` builds the deref chain:

1. Start with `(receiver_ty, [])`.
2. While under `AUTODEREF_LIMIT`:
   - If the current type is `&T` or `*T`, add a built-in `Deref` step to `T`.
   - Otherwise, if `deref_trait` and `deref_target` are registered, try
     `try_deref_target(fcx, current)`.  If the solver can normalize
     `<current as Deref>::Target` with certainty, add a `DerefTrait` step.
   - Otherwise stop.
3. For every step type, emit three probes: by value, by `&`, and by `&mut`.

A `seen` set prevents infinite loops from pathological `Deref` impls.

### 4.2 Speculative `Deref` normalization

`try_deref_target` is purely speculative:

- Snapshot the body `InferCtxt`.
- Create a fresh inference variable `?target`.
- Build the `NormalizesTo` goal
  `<source as Deref>::Target normalizes-to ?target`.
- Canonicalize and evaluate it with `EvalCtxt`.
- If the result is `Certainty::Yes`, apply the solver response to the body
  variables, resolve `?target`, and return the concrete type.
- Roll back the snapshot.

This keeps probing from committing state that may not be used if no method is
found at this deref step.  The returned `target` is interned and stable, so it
can be stored in the `DerefTrait` adjustment.

### 4.3 Candidate assembly

- `pick_inherent_candidate` scans inherent impls whose `self_ty` matches the
  probe type and whose first parameter is a valid receiver (`self`, `&self`,
  `&mut self`).  Unification is tried inside a speculative snapshot.
- `pick_trait_candidate` scans every trait definition for a method with the
  right name.  It builds a fresh substitution for the trait generics plus
  `Self`, checks receiver unification, and returns a `CandidateSource::Trait`
  with the `TraitRef` to prove.

### 4.4 Confirmation

`confirm_method`:

1. Substitutes the impl/trait generics into the raw method signature.
2. Unifies the method's expected receiver with `pick.probe_ty`.
3. For every `DerefTrait { source, target }` adjustment, emits:
   - `NormalizesTo(<source as Deref>::Target == target)`
   - `Trait(source: Deref)`
4. Checks remaining arguments.
5. Emits the candidate's where-clause obligations (inherent) or the trait
   obligation (extension).

`confirm_and_record` then stores the receiver adjustments in
`TypeckResults::expr_adjustments`.

## 5. Solver integration

Method dispatch reuses the same solver machinery as generic trait obligations:

- `EvalCtxt::evaluate_canonical_goal`
- `canonicalize` for the `NormalizesTo` goal
- `apply_response_to_body` to write inferred values back into the body
  `InferCtxt`

The solver's projection normalization (`compute_projection_like_goal`) finds
impls of `Deref`, computes the substituted associated type, and unifies it with
the expected term.  The `impl_item_to_assoc` mapping now matches impl items to
trait items by ident, so `<T as Deref>::Target` resolves correctly even when
only the trait `Target` ID is known.

## 6. Tests

All tests live in `yelang-tycheck/src/tests/mod.rs` and are run with
`cargo test -p yelang-tycheck`.

| Test | Coverage |
|------|----------|
| `inherent_method_call_with_autoref` | Inherent method + `&self` receiver via autoref. |
| `trait_method_call_extension` | Trait extension method + obligation proof. |
| `method_dispatch_via_deref_trait` | One `Deref` step + inherent method + adjustments recorded. |
| `deref_chain_two_steps` | Chained `Deref` steps through two wrappers. |
| `method_not_found_after_autoderef_returns_error` | No applicable method returns `Ty::Error`. |
| `trait_solver_writeback_resolves_infer_var` | Solver writes inferred values back to body infer vars. |

These tests construct HIR directly so they exercise the type checker without
requiring a full parser/front-end.

## 7. Architecture notes

### 7.1 Naming: `Ty` vs `TyKind`, `DefId`, `ItemId`

- `yelang_ty::ty::Ty` is the single recursive type enum; there is no `TyKind`.
  Interned types are referenced by `TyId` from `yelang_arena`.
- `yelang_arena::DefId` is the single typed definition ID.  There is currently
  only one definition namespace; `ItemId` in `yelang-hir` is an alias for it.
  A newtype `ItemId(DefId)` could be added for extra safety, but `DefId` is
  already a typed ID and the whole compiler uses it consistently.
- HIR uses `Item { kind: ItemKind, ... }`.  The `Kind` suffix is appropriate
  because it classifies the payload of a wrapper struct, not a standalone type.

### 7.2 Arena and the `std::mem::take` pattern in `MutVisitor`

The in-place HIR visitor takes nodes out of `Option` slots, visits them, and
puts them back.  This is required by Rust's borrowing rules when both the arena
and the visitor hold `&mut Crate`.  It is not a sign of bad arena usage; it is
the standard way to implement a mutating tree walk.  A fold-style visitor that
returns new nodes could avoid `take`, but that would not be "in place."

### 7.3 Incremental-compilation readiness

The current structures are already incremental-friendly:

- Dense `IndexVec`/`SecondaryMap` keyed by stable IDs for item-level data.
- `slotmap` arenas with generational keys for HIR nodes.
- No interior mutability in `TyCtxt`; all mutation is explicit.
- Solver uses snapshot/rollback probes, which maps cleanly to a query system's
  incremental invalidation.

The next step for true incremental compilation is a Salsa-style query graph
over these tables, not a redesign of the ID/arena scheme.

## 8. What is still left for production-ready type checking / trait solving

### Type checker

- [ ] `_` return-type inference across the collector/body-checker boundary.
- [ ] `HirTy::TypeOf` lowering in the collector.
- [ ] Struct and anonymous-struct field access by name.
- [ ] `Deref` field access (`wrapper.field` resolves through `<Wrapper as Deref>::Target`).
- [ ] Full coercion suite: deref coercion, never coercion, fn-item-to-fn-ptr,
      width subtyping for anon structs, int/float fallback at coercion sites.
- [ ] Span-aware diagnostics with error accumulation (Phase E).
- [ ] Closure checking.
- [ ] Const generics evaluation.

### Trait solver

- [ ] Universe constraints on existential variables.
- [ ] User-facing diagnostics for `NoSolution` and ambiguous goals.
- [ ] Negative trait bounds in user code beyond the current internal polarity.

### Integration

- [ ] Hook the `Deref` lang-item registration into the prelude / lang-item
      collector so tests and the driver do not set it manually.
- [ ] Propagate `expr_adjustments` to MIR lowering and borrow checking.

## 9. References

- rustc dev guide — [Method Lookup](https://rustc-dev-guide.rust-lang.org/hir-typeck/method-lookup.html)
- rustc dev guide — [Trait Resolution](https://rustc-dev-guide.rust-lang.org/traits/resolution.html)
- rustc next-generation trait solver (`rustc_next_trait_solver`)
- Chalk — [Goal-driven trait solver](https://github.com/rust-lang/chalk)
