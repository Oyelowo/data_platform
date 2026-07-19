# Phase I: QIR and Physical Planning Checklist

This checklist tracks the implementation of Phase I. Each item should be
implemented and accompanied by tests before being marked complete.

## 1. Crate and module scaffolding

- [x] Create `crates/compiler/yelang-qir` crate and add it to the workspace.
- [x] Add dependencies: `yelang-arena`, `yelang-hir`, `yelang-ty`,
      `yelang-tycheck`, `yelang-interner`, `yelang-lexer`, `thiserror`.
- [x] Create `src/ids.rs` with `QirId`, `PhysId`, `QExprId` newtypes using
      `yelang_arena::index_vec`.
- [x] Create `src/expr.rs` with `QExpr` and scalar expression forms.
- [x] Create submodule directories: `logical/`, `rewrite/`, `physical/`,
      `exec/`, `backend/`, `util/`.
- [x] Create `tests/lower.rs` (`tests/rewrite.rs`, `tests/plan.rs`,
      `tests/integration.rs` are stubbed in the design doc and will be added
      as their respective phases land).
- [x] Implement `util::arena`, `util::print`, `util::graph` helpers.

## 2. Logical QIR

### 2.1 Data structures

- [x] Define `logical::operator::Operator` enum (Scan, Filter, Map, FlatMap,
      OrderBy, Range, joins, GroupBy, Aggregate, Window, SetOp, Distinct,
      AttachField, Construct, Expr).
- [x] Define `logical::shape::NestedShape` and `logical::shape::CorrelationMode`.
- [x] Define `LogicalPlan` (arena + root id + expression arena).
- [x] Define `LoweringError` enum.

### 2.2 Lowering entry

- [x] `logical::lower::lower_query(tcx, body_id, query_id) -> Result<LogicalPlan, LoweringError>`.
- [ ] Helper to lower a typed HIR expression to `QExpr`.
- [ ] Helper to resolve HIR binder IDs to QIR variable references.

### 2.3 From / scan lowering

- [ ] Lower single `FromNode` to `Scan` + optional `Filter`/`OrderBy`/`Range`.
- [ ] Lower multi-root `from` to `Construct { Facet, fields }`.
- [ ] Preserve auto-call semantics for function-item sources.
- [ ] Lower `for <root> { ... }` post-links modifiers.

### 2.4 Projection lowering

- [ ] Scalar projection -> `Expr(QExpr)`.
- [ ] Per-element projection -> `Map`.
- [ ] Object projection -> struct construction in `QExpr`.

### 2.5 Selector / comprehension lowering

- [ ] `[*]` -> `Map`.
- [ ] `[where p]` -> `Filter`.
- [ ] `[**]` (and deeper) -> `FlatMap`.
- [ ] Combined `base@b[where p][**].suffix` -> single operator chain.
- [ ] Preserve flatten depth in `ComprehensionVar`.

### 2.6 `links` lowering

- [ ] Implement `logical::links::lower_links`.
- [ ] Resolve anchor label to binder/element type.
- [ ] Lower one segment to `LeftOuterJoin` + `AttachField`.
- [ ] Chain multiple segments.
- [ ] Support `->`, `<-`, `<->` via endpoint field selection.
- [ ] Support continuation from intermediate labels.
- [ ] Apply segment-local filters as join predicates or pre-filters.

### 2.7 `group by` lowering

- [ ] Lower keys to `QExpr`.
- [ ] Emit `GroupBy` with `members_label`.
- [ ] Validate single-root key references; reject ambiguous multi-root keys.

### 2.8 Mutation queries

- [ ] Lower `create`, `update`, `upsert`, `delete` to logical mutation
      placeholders.
- [ ] Lower `link`/`unlink` to edge mutation placeholders.
- [ ] Preserve `; <expr>` tail as result `Expr`.

## 3. Rewrite passes

### 3.1 Driver

- [ ] Implement `rewrite::apply_rewrites(plan) -> QirId` with fixed-point loop.
- [ ] Ensure rewrites are idempotent and terminate.

### 3.2 Rules

- [ ] `rewrite::merge_maps`: merge adjacent `Map` operators.
- [ ] `rewrite::push_filter`: push filters into scans and joins when safe.
- [ ] `rewrite::flatten_project`: `Map` over `FlatMap(identity)` => `FlatMap`.
- [ ] `rewrite::lift_links`: merge redundant sibling `links` prefixes.
- [ ] `rewrite::decorrelate`: detect correlated sub-plans via free variables.
- [ ] `rewrite::unnest_subqueries`: transform `DependentJoin` into joins,
      semi/anti joins, or group-joins.

## 4. Physical planning

### 4.1 Properties

- [ ] Define `physical::properties::Properties` with ordering, distribution,
      streaming unit.
- [ ] Compute properties bottom-up for logical operators.
- [ ] Derive required properties from parents.

### 4.2 Operator lowering

- [ ] `Scan` -> `TableScan`.
- [ ] `Filter` -> `Filter`.
- [ ] `Map` -> `Map`.
- [ ] `FlatMap` -> `FlatMap`.
- [ ] `OrderBy` -> `Sort` or no-op.
- [ ] `Range` -> `Slice` or runtime range operator.
- [ ] `InnerJoin`/`LeftOuterJoin` -> `HashJoin`/`MergeJoin`/`NestedLoopJoin`.
- [ ] `SemiJoin`/`AntiJoin`/`MarkJoin` -> physical semi/anti/mark joins.
- [ ] `GroupBy` -> `HashGroupBy` or `SortGroupBy`.
- [ ] `Aggregate` -> `Aggregate`.
- [ ] `Window` -> physical window operator.
- [ ] `SetOp`/`Distinct` -> physical operators.
- [ ] `AttachField` -> folded into `Map` or kept as struct construction.
- [ ] `Construct` -> `Construct`.
- [ ] `Expr` -> `Expr`.

### 4.3 Join selection

- [ ] Choose `HashJoin` for equijoin with small build side or hashable keys.
- [ ] Choose `MergeJoin` when inputs are ordered on join keys.
- [ ] Fall back to `NestedLoopJoin` for arbitrary predicates.

### 4.4 Exchange insertion

- [ ] `physical::exchanges::insert_exchanges(plan, backend)`.
- [ ] `RepartitionBy` for group-by and large joins.
- [ ] `Broadcast` for small build sides.
- [ ] `Gather` for global sorts.

### 4.5 Aggregation selection

- [ ] `physical::aggregations::choose_aggregation`.
- [ ] Decompose aggregates into partial/final forms when backend supports it.

## 5. Execution

### 5.1 Backend capability

- [ ] Define `backend::capability::BackendCapability` trait.
- [ ] Implement `backend::memory_backend::MemoryBackend`.
- [ ] Add `backend::remote_backend::RemoteBackend` placeholder.

### 5.2 Scalar kernels

- [ ] Define `exec::kernels::KernelRegistry`.
- [ ] Register pure scalar operations: arithmetic, comparisons, string ops,
      math functions.

### 5.3 In-memory executor

- [ ] Define `exec::interface::QueryExecutor`.
- [ ] Implement `exec::memory::MemoryExecutor`.
- [ ] Support `TableScan`, `Filter`, `Map`, `FlatMap`, `Sort`, `Slice`,
      `HashJoin`, `NestedLoopJoin`, `GroupBy`, `Aggregate`, `Construct`, `Expr`.

## 6. Testing

### 6.1 Lowering tests (`tests/lower.rs`)

- [ ] Scalar projection returns `Expr`.
- [ ] `select users@u[*].id` -> `Map(Scan, u.id)`.
- [ ] `select users@u[where u.age > 18]` -> `Filter(Scan, predicate)`.
- [ ] `select users@u[*].address.city` -> single `Map`.
- [ ] `select users@u[*].posts@p[**]` -> `FlatMap`.
- [ ] `links` single-hop forward/backward/bidirectional.
- [ ] `links` multi-hop and continuation from intermediate label.
- [ ] Multi-root `from` -> `Construct`.
- [ ] `group by` -> `GroupBy`.
- [ ] Mutation query tail expressions.

### 6.2 Rewrite tests (`tests/rewrite.rs`)

- [ ] Adjacent maps merged.
- [ ] Filter pushed into scan when local.
- [ ] Filter pushed through `LeftOuterJoin` when safe.
- [ ] `Map` over `FlatMap(identity)` fused.
- [ ] Correlated scalar subquery -> `GroupJoin`.
- [ ] Correlated existential subquery -> `SemiJoin`.

### 6.3 Physical-plan tests (`tests/plan.rs`)

- [ ] `HashJoin` chosen for equijoin with small build side.
- [ ] `MergeJoin` chosen when inputs ordered.
- [ ] `NestedLoopJoin` chosen for non-equijoin.
- [ ] `Exchange(Gather)` inserted before global sort on sharded input.
- [ ] `HashGroupBy` chosen for ungrouped input.

### 6.4 Integration tests (`tests/integration.rs`)

- [ ] End-to-end parse + type-check + lower + plan + execute for simple select.
- [ ] Execute against in-memory VM and assert result value.
- [ ] Cover all Phase H integration scenarios at the QIR level.

### 6.5 Workspace regression

- [ ] `cargo test --workspace` passes.
- [ ] No warnings in `yelang-qir` or touched crates.

## 7. Documentation

- [ ] Update `PHASEI_QIR_DESIGN.md` with any implementation-driven changes.
- [ ] Add module-level and public-item doc comments.
- [ ] Add a note in `PHASEH_QUERY_EXPRESSION_DESIGN.md` pointing to Phase I.

## 8. Acceptance criteria

Phase I is complete when:

1. The `yelang-qir` crate compiles and is part of the workspace.
2. Every query expression that passes the Phase H type checker can be lowered
   to a logical QIR plan.
3. Logical rewrites produce equivalent plans and terminate.
4. Physical planning produces a deterministic physical plan for a given backend.
5. The in-memory executor can run the physical plan and produce correct values.
6. All QIR unit tests, rewrite tests, plan tests, and integration tests pass.
7. `cargo test --workspace` passes with no new warnings.
