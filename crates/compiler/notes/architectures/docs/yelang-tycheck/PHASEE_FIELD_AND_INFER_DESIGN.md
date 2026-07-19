# Phase E Design: Field Access, Autoderef Refactor, and `_` Return-Type Inference

This document describes the design and implementation of Phase E of the
`yelang-tycheck` type checker.

## Goals

1. Share the autoderef / autoref probe machinery between method dispatch and
   field access.
2. Implement name-based field lookup for:
   - Named structs (`struct Foo { x: i32 }`)
   - Generic structs, with correct parameter substitution
   - Anonymous structs (`{ x: i32, y: bool }`)
   - Types behind references (`&T`)
   - Types behind user-defined `Deref` impls
3. Implement `_` as an inferred return type in function signatures.
4. Keep the implementation robust enough for the next-gen trait solver and for
   future incremental/IDE work.

## Phase E file tree

```text
yelang-tycheck/
├── Cargo.toml
├── PHASEC_SOLVER_INTEGRATION_DESIGN.md
├── PHASED_METHOD_DISPATCH_DESIGN.md
├── PHASEE_FIELD_AND_INFER_DESIGN.md   # this file
└── src/
    ├── autoderef.rs       # NEW — shared deref chain logic
    ├── check.rs           # field access + `_` return inference
    ├── coerce.rs
    ├── collector.rs       # return_ty_infer detection
    ├── fn_ctxt.rs
    ├── hir_ty_lower.rs
    ├── lib.rs             # registers `mod autoderef`
    ├── lower_ctx.rs
    ├── method.rs          # now uses `crate::autoderef`
    ├── pat.rs
    ├── solver_ctxt.rs
    ├── tcx.rs
    ├── tests/mod.rs       # Phase E tests
    ├── typeck_results.rs  # Adjustment now imported from autoderef
    └── writeback.rs       # fallback commits to inference tables
```

Related crates touched in this phase:

```text
yelang-ty/src/ty.rs   # FnSig now carries `return_ty_infer: bool`
yelang-ty/src/fold.rs # fold preserves `return_ty_infer`
```

## Shared autoderef module (`src/autoderef.rs`)

Method dispatch and field dispatch need exactly the same receiver probing logic:

- Built-in derefs through `&T`, `&mut T`, and raw pointers.
- User-defined derefs through the `Deref` trait, resolved by normalizing
  `<T as Deref>::Target` with the next-generation trait solver.
- A cap of `AUTODEREF_LIMIT = 10` steps to avoid infinite chains.

The module exposes:

- `Adjustment` — a single receiver adjustment:
  - `Deref` — built-in `*`.
  - `Ref` / `RefMut` — `&` / `&mut` (used by method dispatch).
  - `DerefTrait { source, target }` — a user-defined `Deref` step.
- `probe_deref_steps(receiver)` — the ordered deref chain (no autoref).
- `probe_types(receiver)` — the chain plus `&` / `&mut` variants at each step.
- `try_deref_target(source)` — speculative normalization via the solver.
- `emit_deref_trait_obligations(source, target)` — commits the obligations for a
  chosen `DerefTrait` step.

All solver interaction in `try_deref_target` is performed inside an inference
snapshot and rolled back, so probing is side-effect-free. The caller (method or
field dispatch) re-proves the goal when it commits to a step.

## Field access (`src/check.rs::check_field`)

Algorithm:

1. Type-check the base expression.
2. Build the deref chain with `probe_deref_steps`.
3. For each `(probe_ty, adjustments)` in the chain, try `lookup_field`:
   - `Ty::Tuple(args)` — numeric index lookup (existing behavior).
   - `Ty::Adt(def_id, args)` — find the field in the first variant and, if the
     ADT has generic arguments, substitute them into the field type.
   - `Ty::AnonStruct(def)` — find the field by name in the anonymous struct.
4. On the first matching field:
   - Record `adjustments` in `TypeckResults::expr_adjustments` for the base
     expression.
   - Emit `DerefTrait` obligations for any user-defined deref steps used.
   - Return the field type.
5. If no step matches, return `Ty::Error`.

This design gives field access the same deref semantics as method calls without
duplicating the solver-heavy probe logic.

## `_` return-type inference

Function signatures are lowered in the collector before bodies are checked. To
support `fn foo() -> _ { 42 }`:

- `yelang_ty::ty::FnSig` gained a `return_ty_infer: bool` flag.
- `yelang_ty/src/fold.rs` preserves the flag when folding signatures.
- `yelang-tycheck/src/collector.rs::lower_fn_sig` detects `hir::Ty::Infer` in
  the return position and sets the flag (the signature's output is set to
  `Ty::Error` as a placeholder; the real type will come from the body).
- `yelang-tycheck/src/check.rs::check_body` looks up the function's signature
  and, if `return_ty_infer` is set, replaces `fcx.return_ty` with a fresh type
  variable before checking the body.
- The existing `fcx.coerce(body_ty, fcx.return_ty)` then unifies the inferred
  return type with the body expression.

When the body forces the return variable to be an integer/float, the writeback
fallback now commits the fallback (`i32` / `f64`) to the inference tables so that
other references to the same variable (including `fcx.return_ty`) resolve to the
same concrete type.

## Test coverage

Phase E added the following tests in `yelang-tycheck/src/tests/mod.rs`:

| Test | What it covers |
|------|----------------|
| `field_struct_named_access` | Named struct field lookup. |
| `field_struct_missing_is_error` | Missing field returns `Ty::Error`. |
| `field_generic_struct_substitutes_params` | Generic arguments are substituted into field types. |
| `field_anon_struct_access` | Anonymous struct field lookup. |
| `field_through_reference` | Field through `&T` records a `Deref` adjustment. |
| `field_through_deref_trait` | Field through a `Deref` impl records a `DerefTrait` adjustment and proves obligations. |
| `return_type_infer_from_body` | `fn foo() -> _ { 42 }` resolves to `i32`. |

The workspace test suite passes: `cargo test --workspace` reports all green.

## Known limitations and next steps

- `_` return inference currently only affects the body being checked. The
  collected `fn_sigs` table still stores `Ty::Error` for the return type. In a
  full driver the resolved return type should be written back into the item
  signature so that callers see the concrete type.
- Error cases currently return `Ty::Error` but do not yet emit structured
  diagnostics with spans. That belongs to the diagnostics phase.
- Coercion is still mostly exact-match. Deref coercion, never-to-any, fn-item-to-
  fn-ptr, and width subtyping for anonymous structs are listed in the checklist
  as future work.
