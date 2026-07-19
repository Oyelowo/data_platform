# Phase G Design: End-to-End Harness, Diagnostics, and Core Coercions

## Goal

Close the gap between isolated crate unit tests and a usable type-checking
pipeline. This phase delivers:

1. A source-to-diagnostics integration harness.
2. Accumulating, span-carrying diagnostics.
3. Core coercions required before merge:
   - `!` (never) to any type.
   - Function item to function pointer.
   - Deref coercion through references and user-defined `Deref`.
   - Width subtyping for anonymous structs.
   - Integer/float fallback at explicit coercion sites.
4. End-to-end tests that run `parse → resolve → lower → type-check` on raw
   source and assert either success or expected diagnostics.

## What is still missing (post-Phase G)

From `TYCHECK_CHECKLIST.md`:
- `hir::Ty::TypeOf` collector lowering.

From `TRAIT_SOLVER_CHECKLIST.md`:
- Universe constraints on existential variables.
- Advanced solver diagnostics (multi-line notes / suggestions).

All Phase G coercions, the integration harness, accumulating diagnostics, and
Deref lang-item wiring are now implemented.

## Phase G file tree

```text
yelang-tycheck/
├── src/
│   ├── lib.rs                  # re-exports diagnostics + type_check_crate
│   ├── diagnostics.rs          # Diagnostic, Severity, Diagnostic conversion
│   ├── fn_ctxt.rs              # errors, obligation status, symbol-format helpers
│   ├── check.rs                # reports errors; applies coercion at call args
│   ├── coerce.rs               # never, fn-item-to-fn-ptr, deref, width, fallback
│   ├── method.rs               # method dispatch with inherent priority
│   ├── autoderef.rs            # deref-chain probe + DerefTrait obligations
│   ├── type_check_crate.rs     # entry point that checks every body
│   └── tests/mod.rs            # unit tests, including width subtyping
└── tests/
    └── integration.rs          # end-to-end source tests

yelang-resolve/src/lang_items.rs
└── DerefTarget lang item added

yelang-hir/src/crate_data.rs
└── Crate::lang_items registry

yelang-hir/src/lowering/context.rs
└── copies resolved.lang_items into the HIR crate

yelang-hir/src/lowering/item.rs
└── registers DerefTarget when lowering the Target associated type
```

## Diagnostics

`yelang-tycheck` gained a `Diagnostic` type:

```rust
pub struct Diagnostic {
    pub span: Span,
    pub message: String,
    pub severity: Severity,
}
```

`FnCtxt` carries `errors: Vec<(Span, TypeError)>` and reporting helpers:

- `report_type_error(span, error)` — push `(span, error)`.
- `report_mismatch(span, expected, found)` — push `TypeError::Mismatch`.
- `report_obligation_error(span, obligation)` — push a formatted trait error.

`ObligationStatus` distinguishes a true `NoSolution` from an `Ambiguous` result.
When a string interner is supplied, diagnostics resolve `Symbol`s so messages
read `i32`, `Show`, `&mut T` instead of `Id { raw: N }`.

Existing `demand_eq` and coercion failures call these instead of silently
producing `Ty::Error`.

At the end of `check_body`, unresolved inference variables become additional
diagnostics.

## Integration harness

A public function:

```rust
pub fn type_check_crate(tcx: &mut TyCtxt) -> Vec<Diagnostic> {
    // 1. Collect item signatures and register lang items.
    // 2. For every function/const/static body, run check_body.
    // 3. Return collected diagnostics.
}
```

Integration tests live in `yelang-tycheck/tests/integration.rs` and use the
existing helpers from `yelang-resolve` and `yelang-hir`:

```rust
fn type_check_src(src: &str) -> (TyCtxt, Vec<Diagnostic>) { ... }
```

This parses source, resolves names, lowers to HIR, builds a `TyCtxt`, and runs
`type_check_crate`.

## Coercions

`Coerce::coerce(from, to)` now implements:

1. Exact match via unification.
2. If `from` is `Ty::Never`, succeed with `to`.
3. If `from` is `Ty::FnDef` and `to` is `Ty::FnPtr` with a matching signature,
   succeed.
4. Deref coercion: `&T` → `&U` when `T` derefs to `U` through built-in or
   user-defined `Deref`.
5. Width subtyping: an anonymous struct with fields `{a, b}` coerces to a
   structural type requiring `{a}` when the common fields match exactly.
6. Int/float fallback: an integer or float inference variable at a coercion site
   unifies with the concrete target type when that type is integral or floating.

`check.rs` and `method.rs` use coercion at argument sites (function calls and
method calls) rather than plain equality, so deref coercions and numeric
fallbacks apply to actual call arguments.

## Lang-item wiring

The `Deref` trait and its `Target` associated type are now lang items:

- `yelang-resolve/src/lang_items.rs` gained `LangItem::DerefTarget` mapped from
  `"deref_target"`.
- `ResolvedCrate` exposes `lang_items`.
- `yelang-hir::Crate` stores a copy of the registry.
- During HIR lowering of a trait item, if the trait is the `Deref` lang item and
  the item is named `Target`, its synthetic `DefId` is registered as
  `DerefTarget`.
- `yelang-tycheck::collector` reads the registry and calls
  `tcx.register_deref_lang_item(deref_trait, deref_target)`.

This lets `autoderef.rs` build `<T as Deref>::Target normalizes-to U` goals
using the real `DefId`s from the source program.

## Tests

New and updated integration tests (`yelang-tycheck/tests/integration.rs`):

| Test | Source shape | Expected |
|------|--------------|----------|
| `valid_integer_function_has_no_errors` | `fn main() -> i32 { 42 }` | no diagnostics |
| `return_type_mismatch_is_reported` | `fn main() -> i32 { true }` | mismatch diagnostic |
| `missing_field_is_reported` | struct + field access on wrong name | no-such-field diagnostic |
| `trait_not_implemented_is_reported` | method call requiring unimplemented trait | trait bound not satisfied |
| `never_coerces_to_any_type` | `fn die() -> ! { loop {} }` used as `i32` | no diagnostics |
| `fn_item_coerces_to_fn_ptr` | `let f: fn(i32) -> i32 = inc;` | no diagnostics |
| `if_condition_must_be_bool` | `if 1 { }` | mismatch diagnostic |
| `if_branches_must_unify` | `if true { 1 } else { true }` | mismatch diagnostic |
| `call_argument_count_mismatch_is_reported` | `fn f(x: i32) -> i32 { x }` then `f()` | argument count mismatch |
| `call_argument_type_mismatch_is_reported` | `fn f(x: i32) -> i32 { x }` then `f(true)` | type mismatch |
| `integer_literal_coerces_to_annotated_i32` | `let x: i32 = 1;` | no diagnostics |
| `float_literal_coerces_to_annotated_f64` | `let y: f64 = 1.0;` | no diagnostics |
| `inherent_method_takes_priority_over_trait_method` | inherent + trait impl of same method | no diagnostics |
| `mut_self_method_requires_mutable_receiver` | `&mut self` method called on value | no method diagnostic |
| `mut_self_method_works_through_mutable_reference` | `&mut self` method called on `&mut C` param | no diagnostics |
| `deref_trait_coerces_reference_to_target` | `&Wrapper` passed where `&Inner` expected | no diagnostics |
| `method_dispatches_through_deref_trait` | `(&Wrapper).inner_method()` | no diagnostics |

Unit tests in `yelang-tycheck/src/tests/mod.rs` cover width subtyping directly:

- `coerce_anon_struct_width_subtyping`
- `coerce_anon_struct_width_subtyping_field_mismatch_fails`

## Checklist

- [x] Add `Diagnostic` type and severity levels.
- [x] Add `errors` vec to `FnCtxt` and reporting helpers.
- [x] Convert silent `Ty::Error` returns into reported diagnostics where a span
      is available.
- [x] Convert unproven obligations into diagnostics.
- [x] Report unresolved inference variables at end of body.
- [x] Implement `type_check_crate` entry point.
- [x] Create `yelang-tycheck/tests/integration.rs` harness.
- [x] Add end-to-end tests for success and error cases.
- [x] Implement never coercion.
- [x] Implement fn-item-to-fn-ptr coercion.
- [x] Implement deref coercion.
- [x] Implement width subtyping for anonymous structs.
- [x] Implement int/float fallback at coercion sites.
- [x] Wire `Deref` / `DerefTarget` lang items through resolve → HIR → TyCtxt.
- [x] Update `TYCHECK_CHECKLIST.md`.
- [x] `cargo test --workspace` passes.
- [x] Fix HIR lowering local-variable scoping (params/blocks/match arms/lambdas)
      so function parameters do not leak across items.

## Out of scope (post-Phase G)

- `hir::Ty::TypeOf` collector lowering.
- Trait-solver universe constraints.
- Rich diagnostic notes / suggestions / multi-line spans.
- Anonymous-struct literal expressions (the parser supports structural *types*
  but not structural *values* yet; width subtyping is unit-tested with manually
  constructed `Ty::AnonStruct` types).
