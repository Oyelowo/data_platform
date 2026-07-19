# Type Checker Checklist

This checklist tracks the implementation status of Yelang's type checker
(`yelang-tycheck`).

## Global type context (`TyCtxt`)

- [x] `TyCtxt` owns the `Interner`.
- [x] `item_types` table (DefId -> TyId).
- [x] `adt_defs` table.
- [x] `fn_sigs` table.
- [x] `trait_defs` table.
- [x] `impl_defs` table.
- [x] `trait_impl_index` for fast impl lookup.

## Item signature collector

- [x] Walk HIR crate and populate `TyCtxt` tables.
- [x] Lower HIR types to `TyId`, including generic params.
- [x] Lower where clauses to `Predicate`.
- [x] Correct generic parameter indices in `identity_args`.
- [x] Real `DefId`s for impl blocks and trait/impl items.
- [x] Register lang items the type checker needs (e.g. `Deref`, `DerefTarget`).

## HIR type representation

- [x] HIR type node is `yelang_hir::hir::Ty`; semantic type is
      `yelang_ty::Ty`.
- [x] HIR type-node id is `HirTyId`; semantic type id is `TyId`.
- [x] Cross-layer code in `yelang-tycheck` uses `hir::Ty` for the HIR type
      node and unqualified `Ty` for the semantic type.

## `_` inference

- [x] `_` in body type positions lowers to a fresh inference variable.
- [x] `_` in item signatures wires return-type inference across the
      collector/body-checker boundary.

## Body type checking (`FnCtxt`)

- [x] `FnCtxt` holds `&TyCtxt`.
- [x] Expression checking for literals, unary/binary ops, calls, blocks,
      conditionals, loops, matches, returns, field access, indexing, paths.
- [x] Pattern checking for bindings, wildcards, tuples, slices, or-patterns.
- [x] Writeback resolves inference variables and applies int/float fallback.
- [ ] `HirTy::TypeOf` lowering in the collector (currently only in `FnCtxt`).

## Method and field dispatch

- [x] Inherent method lookup with autoref/autoderef.
- [x] Trait (extension) method lookup with autoref/autoderef.
- [x] Prove trait obligations for chosen method.
- [x] Receiver-adjustment side table in `TypeckResults`.
- [x] `Deref`-trait autoderef via projection normalization.
- [x] `Deref` lang-item registration (`deref_trait`, `deref_target`).
- [x] Tuple fields by index.
- [x] Struct/anon-struct fields by name.
- [x] `Deref` field access via `Deref` trait.

## Coercions

- [x] Exact match coercion.
- [x] Never coercion.
- [x] Function item to function pointer.
- [x] Deref coercion.
- [x] Width subtyping for anonymous structs.
- [x] Int/float fallback at coercion sites.

## Trait obligations

- [x] Emit trait obligations after generic calls.
- [x] Prove well-formedness bounds after ADT instantiation.
- [x] Integrate `yelang-trait-solver` into `FnCtxt`.
- [x] Apply solver substitutions back to the body `InferCtxt`.
- [x] Prove explicit where-clause predicates (generic trait-bound args are lowered and used by the solver).

## Diagnostics

- [x] Structured `TypeError` variants.
- [x] Convert `TypeError` and `NoSolution` into user-facing diagnostics with
      spans and notes (basic messages; rich notes/suggestions out of scope).
- [x] Resolve `Symbol`s in diagnostics so types/traits print as `i32`, `Show`,
      `&mut T` instead of raw IDs.
- [x] Distinguish `NoSolution` from ambiguous obligations.
- [x] Error accumulation (do not stop at first error).

## Phase H: Query expressions and array selectors

- [x] `Array<T>` lang item synthesized as a generic HIR struct during lowering.
- [x] Array literals produce `Array<T>` (fallback to fixed-size `Ty::Array`
      when the lang item is unavailable).
- [x] Dynamic array type `[T]` lowered to `Array<T>`.
- [x] Fixed-size array types (`[T; N]`) and repeat expressions (`[value; N]`).
- [x] Single-root `select ... from ...` lowering and type checking.
- [x] `from` source modifiers (`where`, `order by`, `range`).
- [x] Top-level query tail clauses (`where`, `order by`, `range`).
- [x] Array selectors `[*]`, `[where ...]`, `[**]` lowered to
      `Expr::Comprehension`.
- [x] Selector chains and nested field/method access folded into a single
      comprehension.
- [x] Auto-call zero-arg function items used as selector/query sources.
- [x] Array builtins: `len`, `count`, `is_empty`, `any`, `all`.
- [x] Mutation queries (`create`, `update`, `upsert`, `delete`, `link`,
      `unlink`) with object payloads, `set`/`merge`, `where`, and `; <expr>`
      tail clauses.
- [x] `_` return-type inference driven by query/array bodies.
- [x] Exhaustive positive and negative integration tests in
      `yelang-tycheck/tests/integration.rs`.
- [x] Design doc: `PHASEH_QUERY_EXPRESSION_DESIGN.md`.

## Deferred

- [ ] `links` graph traversal lowering and type checking.
- [ ] `group by` lowering and type checking.
- [ ] Multi-root `from` and `for <root> { ... }` modifiers.
- [ ] Selector-local `order by`, slicing, `group by`, `distinct`, `enumerate`.
- [ ] Closure signature checking for `any` / `all` predicates.

## Testing

- [x] Unit tests for `TyCtxt`/collector.
- [x] Body-checker tests for expressions, patterns, coercions, writeback.
- [x] End-to-end tests for generic functions, structs, enums, traits, impls,
      associated types (Phase C coverage; more in Phase D/E).
- [x] Error-case tests with expected diagnostics via
      `yelang-tycheck/tests/integration.rs`.
- [x] Phase H query/selector integration tests.
