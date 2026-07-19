# Yelang Type System Design

This document describes the architecture, design decisions, and current status of the Yelang type system — the `yelang-ty`, `yelang-tycheck`, and `yelang-trait-solver` crates — and how they connect to `yelang-hir` and `yelang-resolve`.

It is written for a greenfield compiler with no backwards-compatibility burden. We therefore choose the architecture that is correct and maintainable from the start, drawing from rustc's next-generation trait solver (a.k.a. the "recursive solver"), rust-analyzer's query-based design, and Chalk.

## 1. Goals and non-goals

### Goals

- Sound, complete, and scalable type checking for a Rust/Zig-inspired surface language.
- Parametric generics with explicit where clauses.
- Associated types and projection normalization.
- Coinductive auto traits and negative/positive polarity impls.
- A next-generation recursive trait solver with canonicalization, caching, and cycle handling.
- A clean separation between syntactic HIR types, canonical type-system IR, and inference state.
- Incremental-friendly data structures (dense IDs, side tables, immutable interned IR).

### Non-goals (for this phase)

- Lifetimes / borrow checking. Yelang is lifetime-free at the type-system level.
- Higher-ranked trait bounds (HRTB) are represented but not fully solved yet.
- Const generics evaluation beyond representation.
- Variance / subtyping beyond coercion at specific sites.

## 2. High-level pipeline

```
AST  --(resolve)-->  ResolvedCrate
                     |
                     v
              HIR (yelang-hir)
                     |
                     v
            collector --(hir_ty_lower)-->  TyCtxt tables
                     |
                     v
            body type checker (FnCtxt)
                     |
                     +--> unify via InferCtxt (yelang-infer)
                     +--> prove obligations via trait solver
                     |
                     v
            writeback / TypeckResults
```

1. **Name resolution** (`yelang-resolve`) assigns a single dense `DefId` to every definition: items, generic parameters, enum variants, fields, impl blocks, trait/impl/foreign items.
2. **HIR lowering** (`yelang-hir`) desugars syntax and produces `Crate`, where items live in `IndexVec<DefId, Option<Item>>` and expression/pattern/statement/type/body nodes live in generational `Arena<Id, Option<T>>`.
3. **Type collection** (`yelang-tycheck::collector`) walks HIR items once and populates `TyCtxt` with signatures, ADT definitions, generics, trait defs, and impl blocks.
4. **Body type checking** (`yelang-tycheck::check`, `fn_ctxt`, `pat`, `coerce`) checks each function body using a per-body `InferCtxt`, emits trait/well-formedness obligations, and records expression/pattern types in `TypeckResults`.
5. **Trait solving** (`yelang-trait-solver`) proves obligations in isolated, canonicalized goals.
6. **Writeback** (`yelang-tycheck::writeback`) resolves inference variables and applies integer/float fallback.

## 3. File tree

### `yelang-hir` (syntactic IR)

```
yelang-hir/src/
  lib.rs                    crate re-exports
  ids.rs                    DefId re-export, ExprId, PatId, StmtId, HirTyId, BodyId, ItemId
  res.rs                    Res, PrimTy, IntTy, FloatTy
  crate_data.rs             Crate root + Arena/IndexVec storage + node lookup helpers
  map.rs                    HIR Map (id -> node)
  validate.rs               HIR validation pass
  hir/
    mod.rs                  re-exports
    core.rs                 Item, TraitItem, ImplItem, ForeignItem, Generics, FnSig, Stmt, etc.
    item.rs                 ItemKind
    expr.rs                 Expr
    ty.rs                   Ty (syntax), GenericArg, Const, UtilityKind
    pat.rs                  Pat
    body.rs                 Body, Param
    adt.rs                  VariantData, FieldDef, StructField
  lowering/                 AST -> HIR lowering
  visit/                    visitor, mut_visitor, folder
  derive/                   built-in derive expansion
  tests/                    exhaustive lowering/storage/derive/visitor tests
```

### `yelang-ty` (canonical type IR)

```
yelang-ty/src/
  lib.rs                    re-exports
  ty.rs                     Ty, TyId, Const, ConstId, inference variables, ADT/fn/tuple/ref/etc.
  interner.rs               hash-consing arena for types, constants, lists
  generic.rs                GenericArg
  predicate.rs              Predicate, ParamEnv, TraitRef, TraitPredicate
  primitive.rs              IntTy, UintTy, FloatTy
  projection.rs             ProjectionTy, AliasTy
  existential.rs            ExistentialPredicate, trait objects
  binder.rs                 BoundVar, DebruijnIndex, Binder
  consts.rs                 ConstValue, ParamConst, UnevaluatedConst
  list.rs                   interned `List<T>`
  fold.rs / visit.rs        structural traversal
  subst.rs                  substitution
  canonical.rs              Canonical<T>, CanonicalVarInfo
```

### `yelang-infer` (unification)

```
yelang-infer/src/
  context.rs                InferCtxt: ty/int/float/const vars, unification
  error.rs                  TypeError variants
  type_variable.rs          TyVarValue, IntVarValue, FloatVarValue, ConstVarValue
  subst.rs                  apply substitutions
```

### `yelang-trait-solver` (next-gen solver)

```
yelang-trait-solver/src/
  lib.rs                    re-exports
  solver_ctx.rs             SolverCtxt trait (adapter interface)
  eval_ctxt.rs              EvalCtxt: recursive solver core
  goal.rs                   Goal, CanonicalGoal
  response.rs               CanonicalResponse, Certainty
  search_graph.rs           evaluation stack + cache, cycle detection
  candidate.rs              candidate assembly
  canonicalize.rs           canonicalization of goals
  instantiate.rs            instantiation of canonical responses
  normalize.rs              projection normalization
  builtin.rs                Sized, Copy, Clone, auto traits
  tests/                    canonicalization, solver, builtin, cycle tests
```

### `yelang-tycheck` (driver)

```
yelang-tycheck/src/
  lib.rs                    re-exports
  tcx.rs                    TyCtxt global tables
  collector.rs              collect item signatures from HIR
  hir_ty_lower.rs           lower HIR `Ty` -> canonical `TyId`
  lower_ctx.rs              TyLowerCtxt trait
  fn_ctxt.rs                FnCtxt body checking context
  check.rs                  expression/statement type checking
  pat.rs                    pattern type checking
  coerce.rs                 coercion logic
  method.rs                 method lookup (placeholder)
  solver_ctxt.rs            SolverCtxt impl for TyCtxt
  writeback.rs              resolve inference variables
  typeck_results.rs         TypeckResults per body
  tests/                    unit tests
```

## 4. Key design decisions

### 4.1 One `DefId` namespace

`yelang-arena::DefId` (`Id<TagDef>`) is the single global identifier for every definition-like entity. There is no separate `ItemId`, `VariantId`, `FieldId`, etc. as newtypes.

Rationale:
- Keeps name resolution, HIR, and the type system in one dense namespace.
- Avoids conversion boilerplate where the same entity flows through item, generic, and field lookups.
- `ItemId` is a semantic alias (`pub type ItemId = DefId`) for documentation only.

For cross-crate support in the future, `DefId` will become `{ krate: CrateNum, index: u32 }`. The current `Id<TagDef>` is a deliberate temporary simplification.

### 4.2 HIR item payloads are inlined

`Item`, `TraitItem`, `ImplItem`, and `ForeignItem` own their `*Kind` enum directly:

```rust
pub struct Item {
    pub def_id: DefId,
    pub ident: Ident,
    pub kind: ItemKind,
    pub vis: Visibility,
    pub attrs: Vec<Attribute>,
    pub span: Span,
}
```

There are no `ItemKindId` / `ItemKind` arenas. This removes the `std::mem::replace(slot, dummy)` hack from item visitors and makes `IndexVec<DefId, Option<Item>>` the single source of truth for an item.

### 4.3 HIR type-syntax ID is `HirTyId`

The HIR type node ID is named `HirTyId` rather than `TyId`. The canonical type-system ID is `yelang_ty::TyId` (re-exported from `yelang_arena::TyId`).

Rationale:
- Prevents the confusing `HirTyId` vs `TyId` naming pattern.
- Makes import sites self-documenting: `yelang_hir::ids::HirTyId` is syntax; `yelang_ty::TyId` is the interned semantic type.

### 4.4 Slotmap arenas use `Option<T>`

`Crate::exprs`, `pats`, `stmts`, `tys`, and `bodies` are `Arena<Id, Option<T>>`. This lets mutating visitors use `std::mem::take(slot)` and put the node back with `*slot = Some(node)`, eliminating dummy-value replacement. It also gives a natural representation for invalidated nodes in incremental compilation.

Lookup helpers on `Crate` flatten the double option:

```rust
pub fn expr(&self, id: ExprId) -> Option<&Expr> {
    self.exprs.get(id).and_then(|o| o.as_ref())
}
```

### 4.5 Canonical type IR is interned and lifetime-free

`yelang_ty::Ty` is hash-consed in `Interner`. `TyId` and `ConstId` are `Copy` 4-byte IDs. Equality is `TyId` equality. There are no lifetimes and no subtyping at the IR level.

### 4.6 Next-generation recursive trait solver

The solver is modeled on rustc's new solver and Chalk:

- **Canonicalization**: Goals are canonicalized (free inference vars become bound vars) before solving so the cache is stable under variable renaming.
- **Search graph**: Separate evaluation stack (for cycle detection) and global cache (keyed by `CanonicalGoal`).
- **Cycle handling**: Coinductive cycles iterate to fixpoint; inductive cycles return `Maybe` (overflow).
- **Candidate assembly**: Param-env, user impls, built-in traits, auto traits.
- **Projection normalization**: `<T as Trait>::Assoc` is solved via `NormalizesTo` and the defining impl.

The solver is decoupled from HIR via the `SolverCtxt` trait. `TyCtxt` implements `SolverCtxt` and precomputes solver-facing views in `populate_solver_caches`.

### 4.7 Inference and solver `InferCtxt`s are separate

The body checker uses one `InferCtxt` for unification. The solver uses its own `InferCtxt` probes for each candidate. Proven canonical responses are not yet applied back to the body `InferCtxt` — this is explicitly listed as remaining work.

## 5. Current status

### Implemented

- `TyCtxt` tables: item types, ADT defs, fn sigs, generics, trait defs, impl defs, impl index.
- HIR -> canonical type lowering including generic params, where clauses, and generic trait-bound arguments (`T: Foo<U>`).
- Body type checking for literals, unary/binary ops, calls, blocks, conditionals, loops, matches, returns, field/index access, paths, patterns.
- Inference variables, unification, int/float fallback.
- Writeback.
- Trait obligation emission and solver integration.
- Solver result writeback into the body `InferCtxt` (inferred types from trait goals).
- Next-gen solver: canonicalization, search graph, candidate assembly, projection normalization, built-in traits, auto traits, coinductive cycles, overflow.

### Remaining (tracked in checklists)

From `TYCHECK_CHECKLIST.md`:

- `_` inference across collector/body-checker boundary for item signatures.
- `HirTy::TypeOf` lowering in the collector.
- Inherent and trait method lookup with autoref/autoderef.
- Tuple, struct/anon-struct, and `Deref` field access.
- Coercions: deref, never, fn-item-to-fn-ptr, width subtyping, int/float fallback at coercion sites.
- User-facing diagnostics with spans and error accumulation.

From `TRAIT_SOLVER_CHECKLIST.md`:

- Universe constraints on existential variables.
- Solver diagnostics for `NoSolution` and ambiguity.

## 6. Testing strategy

Each crate has exhaustive unit tests. The principle is: if a feature is implemented, it has tests; if it is not implemented, the checklist marks it explicitly.

- `yelang-ty`: tests for interning, folding, generic args, canonicalization prerequisites.
- `yelang-infer`: tests for unification, variable resolution, error cases.
- `yelang-trait-solver`: tests for canonicalization, simple/generic impls, param-env, projection, auto traits, coinductive cycles, inductive overflow, negative impls, supertraits, blanket impls, ambiguity.
- `yelang-tycheck`: tests for collector, body checking, coercions, writeback, generic calls, trait obligations.
- `yelang-hir`: tests for lowering, storage invariants, derives, visitors, types.

Future error-case tests will assert expected diagnostics (span, message, notes) rather than just `is_err()`.

## 7. References

- rustc dev guide — trait solver: <https://rustc-dev-guide.rust-lang.org/solve/trait-solving.html>
- rustc dev guide — caching: <https://rustc-dev-guide.rust-lang.org/solve/caching.html>
- rustc dev guide — coinduction: <https://rustc-dev-guide.rust-lang.org/solve/coinduction.html>
- Chalk book — recursive solver: <https://rust-lang.github.io/chalk/book/recursive/search_graph.html>
- rust-analyzer HIR design (query-based, lifetime-free IDs).

## 8. Related documents

- `yelang-hir/SPEC.md` — HIR structure and ID/arena rationale.
- `yelang-hir/HIR_CHECKLIST.md` — HIR feature checklist.
- `TYCHECK_CHECKLIST.md` — type checker checklist.
- `TRAIT_SOLVER_CHECKLIST.md` — trait solver checklist.
- `yelang-ty/TY_IR_CHECKLIST.md` — type IR checklist.
- `yelang-ty/ID_REFACTOR_DESIGN.md` — type IR ID refactor design.
- `yelang-trait-solver/PHASE4_SOLVER_CORE_DESIGN.md` — solver core design.
- `yelang-trait-solver/PHASE5_CANDIDATE_ASSEMBLY_DESIGN.md` — candidate assembly design.
- `yelang-tycheck/PHASEC_SOLVER_INTEGRATION_DESIGN.md` — solver integration design.
