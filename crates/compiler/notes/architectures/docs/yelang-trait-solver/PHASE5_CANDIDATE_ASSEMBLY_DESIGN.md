# Phase 5 — Candidate Assembly and Projection Normalization

This document is the authoritative design for the remaining candidate kinds
and for associated-type normalization in `yelang-trait-solver`. It is written
for a greenfield implementation and avoids legacy approaches.

## Sources and prior art

- [Goals and clauses — Rust Compiler Development Guide](https://rustc-dev-guide.rust-lang.org/traits/goals-and-clauses.html)
- [Coinduction — Rust Compiler Development Guide](https://rustc-dev-guide.rust-lang.org/solve/coinduction.html)
- [The Chalk book: associated types](https://rust-lang.github.io/chalk/book/clauses/associated_types.html)
- [trait-system-refactor-initiative #1: normalizes-to bound](https://github.com/rust-lang/trait-system-refactor-initiative/issues/1)
- [trait-system-refactor-initiative #223: remove NormalizesTo/Projection split](https://github.com/rust-lang/trait-system-refactor-initiative/issues/223)

## Goals of this phase

1. **Associated-type normalization** — prove `<T as Trait>::Assoc normalizes-to U`
   by selecting the unique applicable impl and returning the substituted
   associated type.
2. **Projection equality** — prove `<T as Trait>::Assoc == U` by normalizing the
   projection to `V` and unifying `V == U`.
3. **Auto-trait derivation** — derive auto traits structurally from ADT fields,
   tuple elements, array elements, references, etc., with coinductive cycle
   handling.
4. **Blanket impls** — fully evaluate generic `impl<T> Trait for T where …`
   candidates.
5. **Negative impls / polarity** — handle `ImplPolarity::Negative` in user impls
   and param-env assumptions.
6. **Supertrait elaboration** — when a trait goal succeeds, also prove its
   supertraits.
7. **Lazy nested goals (ambiguity stalling)** — allow a candidate to return
   `Maybe` when a nested goal is ambiguous, and keep a record of the nested
   goals instead of eagerly failing.

## What this phase does NOT do

- Higher-ranked type / lifetime reasoning (Yelang has no lifetimes).
- Opaque-type normalization (`impl Trait` / type aliases) — kept in a slot for
  Phase 6/7.
- Coherence / overlap checking beyond simple ambiguity.
- Integration with `TyCtxt` — that is Phase 6.

## File tree

```
yelang-trait-solver/
├── PHASE4_SOLVER_CORE_DESIGN.md      <- completed
├── PHASE5_CANDIDATE_ASSEMBLY_DESIGN.md   <- this file
├── TRAIT_SOLVER_CHECKLIST.md
└── src/
    ├── lib.rs
    ├── solver_ctx.rs                 <- extended with assoc items + ADT fields
    ├── search_graph.rs
    ├── eval_ctxt.rs                  <- projection, auto, blanket, supertraits
    ├── candidate.rs
    ├── builtin.rs
    ├── normalize.rs                  <- real normalization helper
    ├── canonicalize.rs
    ├── instantiate.rs
    ├── goal.rs
    ├── response.rs
    └── tests/
        ├── mod.rs
        ├── canonicalize.rs
        ├── builtin.rs
        ├── solver.rs                 <- Phase 4 tests + Phase 5 tests
        └── support.rs                <- TestCtxt extended for assoc types/ADTs
```

## Core data structures

### `SolverCtxt` extensions

```rust
pub trait SolverCtxt<'tcx> {
    // ... existing methods ...

    /// Associated items of a trait definition.
    fn trait_assoc_items(&self, trait_def_id: DefId) -> &[AssocItemInfo<'tcx>];

    /// Associated items of an impl block.
    fn impl_assoc_items(&self, impl_def_id: DefId) -> &[AssocItemInfo<'tcx>];

    /// Field types of an ADT, for auto-trait derivation.
    fn adt_field_tys(&self, adt_def_id: DefId) -> &[Ty<'tcx>];
}
```

### `AssocItemInfo<'tcx>`

```rust
pub struct AssocItemInfo<'tcx> {
    pub def_id: DefId,
    /// For an impl assoc item, the def id of the corresponding trait item.
    /// For a trait assoc item this is `None` (the item is its own trait item).
    pub trait_item_def_id: Option<DefId>,
    pub ident: Symbol,
    pub kind: AssocItemKind<'tcx>,
}

pub enum AssocItemKind<'tcx> {
    Type {
        bounds: Vec<TraitRef<'tcx>>,
        default: Option<Ty<'tcx>>,
    },
    Fn { sig: PolyFnSig<'tcx> },
    Const { ty: Ty<'tcx> },
}
```

Only the `Type` variant matters for normalization in this phase. The
`trait_item_def_id` field allows robust mapping from an impl associated type to
its trait-level item, falling back to name matching when unavailable.

## Normalization

### `NormalizesTo` goal

A goal of the form `<T as Trait>::Assoc normalizes-to U` is proved as follows:

1. Assemble candidates for `T: Trait` (param-env, user impls, built-ins, auto).
2. In a probe, for each candidate that makes `T: Trait` hold, compute the
   associated type `V` from the selected impl:
   - For a user impl, find the `Type` assoc item with the same `item_def_id`,
     apply the impl substitution, and return `V`.
   - Built-in / auto traits have no associated types, so they cannot prove
     `NormalizesTo`.
3. If exactly one candidate yields `Yes` with a normalized type `V`, unify
   `V == U` and commit the candidate.
4. If multiple candidates yield `Yes` with distinct `V`, return `Maybe`.
5. If no candidate applies, return `NoSolution`.

### `Projection` equality goal

A goal `<T as Trait>::Assoc == U` is proved by:

1. Trying to normalize `<T as Trait>::Assoc` to `V` and unifying `V == U`.
2. If normalization is not possible (no applicable impl), return `Maybe`.

In the current Yelang IR there is no separate placeholder associated-type
reasoning, so projection equality reduces to normalization.

## Auto-trait derivation

Auto-trait candidates are derived structurally. When both a user-written impl
and the auto-trait derivation would succeed for the same `Self` type, the
solver prefers the user-written impl. This mirrors Rust's behavior where an
explicit `impl Send for Foo` disables automatic `Send` derivation for `Foo`.

An auto-trait candidate is derived structurally:

- **ADT**: add nested goals `Field_i: Auto` for every field, substituting the
  ADT's generic arguments into the field types first.
- **Tuple**: add nested goals for every element.
- **Array**: add nested goal for the element type.
- **Reference** (`&T`, `&mut T`): add nested goal `T: Auto`.
- **Raw pointer**: add nested goal `T: Auto`.
- **Fn pointer / fn item**: `Yes` for `Send`/`Sync`-like traits; for `Sized` use
  built-in rules.
- **Projection / alias**: try to normalize, then derive on the normalized type.
- **Infer / Param / Placeholder / Bound**: `Maybe`.
- **Never / Error**: `Yes`.

Because auto traits are coinductive, cycles through recursive ADTs iterate to
`Yes` (already handled by the Phase 4 cycle logic).

## Blanket impls

A blanket impl has `Self` as a generic parameter, e.g.
`impl<T: Clone> Clone for Option<T>`. Our existing user-impl machinery already
handles this: fresh inference vars are created for the impl generics, the
impl's `trait_ref` (including `Self`) is unified with the goal, and the impl's
where-clauses are added as nested goals. The only extra work is ensuring that
blanket impls are not accidentally treated as built-ins and that overlap with
built-ins is handled (built-in + blanket both `Yes` → `Maybe`).

## Negative impls and polarity

`ImplPolarity::Positive` and `ImplPolarity::Negative` must be handled in three
places:

1. **Param-env assumptions**: a negative assumption `T: !Trait` can prove a
   negative goal `T: !Trait`.
2. **User impls**: a negative impl `impl !Trait for T` proves `T: !Trait`.
3. **Goal polarity**: when proving a positive goal, only positive candidates
   apply; when proving a negative goal, only negative candidates apply.

A negative goal succeeds if a matching negative impl/assumption applies. We do
**not** prove a negative goal by the absence of a positive impl — that is
unsound without coherence.

## Supertrait elaboration

Trait definitions store supertraits in `TraitDefInfo::supertraits`. After a
trait goal succeeds (regardless of which candidate was selected), the solver
adds nested goals for every supertrait, with the same `Self` type substituted
into the supertrait `TraitRef`.

Example: `trait Foo: Bar {}` and goal `T: Foo`. The selected impl proves
`T: Foo`; the solver then also requires `T: Bar`.

## Lazy nested goals / ambiguity stalling

In Phase 4 every nested goal had to be `Yes` for a candidate to succeed. That
is too strict for projection normalization and auto traits in the presence of
unresolved inference variables. In Phase 5 a nested goal that returns `Maybe`
stalls the candidate to `Maybe` instead of failing it. The candidate records
its remaining nested goals; the solver returns `Maybe` to the caller, which can
retry after more type information is available.

For Phase 5 we keep the eager-evaluation loop but correctly propagate `Maybe`
as ambiguity. A full deferred-goal queue is left for Phase 6/7.

## Unresolved inference variables as ambiguity

When a trait goal has an unresolved `TyVar` as its `Self` type and no candidate
applies, the solver returns `Maybe` rather than `NoSolution`. The variable may
later be resolved to a type that implements the trait, so the goal is genuinely
ambiguous at this point. This is essential for blanket impls and generic code
where `Self` is not yet known.

## Testing strategy

Every case has a dedicated test in `src/tests/solver.rs`:

- normalization through a simple impl,
- normalization through a generic impl,
- normalization failure (no applicable impl),
- projection equality via normalization,
- auto-trait derivation for ADT,
- auto-trait derivation for tuple,
- auto-trait derivation for reference,
- auto-trait derivation for nested generic ADT,
- auto-trait coinductive cycle through recursive ADT,
- user impl preferred over auto-trait derivation when both apply,
- blanket impl,
- negative impl proves negative goal,
- negative goal does not succeed without negative impl,
- supertrait elaboration,
- ambiguous nested goal stalls to `Maybe`.

Tests use `TestCtxt` extended with associated-type items and ADT field tables.
