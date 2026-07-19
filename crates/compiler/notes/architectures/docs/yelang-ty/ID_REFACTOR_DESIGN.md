# Type-System ID Refactor Design

## Summary

The core type-system IR (`yelang-ty`), inference engine (`yelang-infer`), next-generation trait solver (`yelang-trait-solver`), and type checker (`yelang-tycheck`) have been refactored so that `Ty` and `Const` are the recursive enums of type-system constructors, while `TyId` and `ConstId` are small, lifetime-free interned IDs backed by dense side tables in `Interner`.

Before this refactor, the codebase used the `TyKind`/`ConstKind` naming convention: `Ty` was an interned ID and `TyKind` was the enum of constructors. After the refactor, the constructors are named `Ty` and `Const` (matching the user's request and the common Rust convention where `Ty` is the type itself), and the interned IDs are `TyId` and `ConstId`:

- `Ty` is the recursive enum of all type constructors (`Bool`, `Int`, `Adt`, ...).
- `TyId` (`Id<TagTy>`) is the interned ID of a `Ty`.
- `Const` is the recursive enum of all const constructors (`Value`, `Param`, `Infer`, ...).
- `ConstId` (`Id<TagConst>`) is the interned ID of a `Const`.
- `Interner` stores `IndexVec<TyId, Ty>` and `IndexVec<ConstId, ConstData>` with hash-consing.
- All dependent crates are lifetime-free in their type-system data.

## Goals

1. **Incremental compilation readiness**: IDs are stable, cheap to hash, and easy to serialize. Dense `IndexVec` side tables are ideal for persistent query contexts.
2. **No lifetime gymnastics**: Removing `<'tcx>` eliminates an entire class of lifetime bugs and makes cross-crate metadata and caching simpler.
3. **No pointer duality**: `TyId` is the ID; `Ty` is the data. The relationship is explicit instead of hidden behind a reference.
4. **Arena selection per use case**:
   - `IndexVec<TyId, Ty>` for types: dense, never removed, cache-friendly.
   - `IndexVec<ConstId, ConstData>` for constants: same rationale.
   - `bumpalo::Bump` for interned lists: immutable, hash-consed slices.

## What did not change

- `Ty` remains a recursive enum. Types are inherently recursive; flattening every subcomponent into IDs would make the code far more verbose with no benefit.
- `List<T>` remains an interned slice pointer. Lists are already hash-consed and stable.
- Bumpalo is kept for interned lists and small arena allocations; it is not replaced by slotmap because types/lists are never removed individually.

## Data layout

```rust
// yelang-arena/src/id.rs
pub struct TagTy;
pub struct TagConst;
pub type TyId = Id<TagTy>;
pub type ConstId = Id<TagConst>;

// yelang-ty/src/ty.rs
pub enum Ty { /* ... lifetime-free constructors ... */ }
pub enum Const { /* ... lifetime-free constructors ... */ }
pub struct ConstData { kind: Const, ty: TyId }

// yelang-ty/src/interner.rs
pub struct Interner {
    types: RefCell<IndexVec<TyId, Ty>>,
    type_map: RefCell<FxHashMap<Ty, TyId>>,
    consts: RefCell<IndexVec<ConstId, ConstData>>,
    const_map: RefCell<FxHashMap<ConstData, ConstId>>,
    arena: bumpalo::Bump,
    // list interning tables ...
}
```

## Key APIs

- `Interner::ty(ty_id: TyId) -> &Ty` returns a reference to the constructor enum.
- `Interner::mk_ty(&self, kind: Ty) -> TyId` hash-conses and returns the ID.
- `Interner::const_kind(ct_id: ConstId) -> &Const` returns a reference to the const constructor.
- `Interner::const_ty(ct_id: ConstId) -> TyId` returns the type of a constant.
- `Interner::mk_const_from_parts(&self, kind: Const, ty: TyId) -> ConstId` convenience.

## Traversal

`TypeFolder` and `TypeVisitor` are lifetime-free and expose `fn interner(&self) -> &Interner`. `TypeFolder::fold_ty` takes and returns `TyId`; `TypeFolder::fold_const` takes and returns `ConstId`. All folding and visiting reconstructs types through the interner so the hash-consing invariant is preserved.

`SubstFolder` applies substitutions recursively: if a replacement type contains parameters that are also being substituted, they are folded again. This enables simultaneous substitutions such as `[T -> Vec<U>, U -> i64]`.

## Migration notes

- Use `Ty` for type constructors and `TyId` for interned type IDs.
- Use `Const` for const constructors and `ConstId` for interned const IDs.
- Replace `ty.kind(interner)` with `interner.ty(ty)`.
- Replace `ct.kind(interner)` with `interner.const_kind(ct)`.
- Replace `ct.ty(interner)` with `interner.const_ty(ct)`.
- `Const { kind, ty }` literals must be created via `interner.mk_const_from_parts(kind, ty_id)`.
- `InferCtxt::eq` and `InferCtxt::eq_const` take `interner: &Interner` as the first argument and operate on `TyId`/`ConstId`.
- `occurs_check` takes `interner: &Interner` as the first argument.
- `Canonical<'tcx, T>` became `Canonical<T>`; `EvalCtxt<'tcx, C>` became `EvalCtxt<'a, C>` keeping only the reference lifetime for the interner/context references.

## Files touched

- `yelang-ty/src/ty.rs` — `Ty` enum, `Const` enum, `ConstData`, re-exports.
- `yelang-ty/src/interner.rs` — `IndexVec<TyId, Ty>`, `IndexVec<ConstId, ConstData>`, `ty`, `const_kind`, `const_ty`.
- `yelang-ty/src/fold.rs` — `TypeFolder` folds `TyId`/`ConstId`.
- `yelang-ty/src/visit.rs` — `TypeVisitor` visits `TyId`/`ConstId`.
- `yelang-ty/src/subst.rs` — `SubstFolder` works with `TyId`/`ConstId`.
- `yelang-ty/src/tests/*` — updated to new API.
- `yelang-infer/src/*` — inference tables store `TyId`/`ConstId`.
- `yelang-trait-solver/src/*` and `src/tests/*` — canonicalization, instantiation, solver, tests.
- `yelang-tycheck/src/*` — collector, HIR type lowering, `FnCtxt`, writeback, solver context, tests.
- `yelang-trait-solver/src/solver_ctx.rs` — cleaned up unused import.
- `yelang-hir/src/hir/adt.rs` — cleaned up unused import.
- `yelang-ty/ID_REFACTOR_DESIGN.md` (this doc) and `yelang-ty/TY_IR_CHECKLIST.md`.
- `TRAIT_SOLVER_CHECKLIST.md`, `TYCHECK_CHECKLIST.md`, `yelang-tycheck/PHASEC_SOLVER_INTEGRATION_DESIGN.md`.

## Testing

The refactor is covered by:

- Interner deduplication tests for types and constants.
- Identity and param-substitution folding tests, including ADT, projection, and anonymous-struct cases.
- Visiting tests for nested types, projections, and dynamic trait objects.
- Substitution tests for type and const params, including nested substitution.
- Binder shifting and instantiation tests.
- Full workspace test suite (`cargo test --workspace`) with all crates passing.
