# Phase I: Query Intermediate Representation (QIR), Physical Planning, and Storage-Aware Surface

This document is the design for lowering the typed HIR query constructs
(completed in Phase H) into a Query Intermediate Representation (QIR), applying
logical rewrites, planning physical operators, and executing against pluggable
storage backends. It is the output of the design review in
`agents/main/plans/luke-cage-shazam-jessica-cruz.md`.

## Status

Phase H is complete. Phase I is the next milestone.

## Goals

1. Give every query expression a stable, typed, optimizer-friendly IR that
   decouples surface syntax from execution strategy.
2. Preserve Phase H semantics exactly:
   - projection defines the result value/shape,
   - `links` materializes nested arrays under upstream elements,
   - `group by` produces a new collection of group objects,
   - multi-root `from` produces independent facets.
3. Support logical rewrites (predicate push-down, decorrelation, join
   reordering, redundant-traversal elimination) without touching HIR or AST.
4. Produce a physical plan with explicit operators, exchange boundaries, and
   streaming units so the VM can execute in-memory or distributed.
5. Keep the IR incremental-friendly: QIR nodes are keyed by typed HIR
   expression/body IDs so that later incremental compilation can reuse
   unchanged sub-plans.
6. Keep Yed a general-purpose language: queries are expressions, methods, and
   functions. QIR recognizes standard-library identities, not user spellings.

## Non-goals for Phase I

- Real distributed cluster execution (the sharded runtime is Phase J/K).
- Cost-based optimization beyond simple heuristics.
- Adaptive / runtime re-optimization.
- General SQL-style `JOIN` surface syntax (still expressed through `links`
  and explicit collection methods).

## File tree

```text
crates/compiler/yelang-qir/
├── Cargo.toml
└── src/
    ├── lib.rs                        # crate facade, query driver, errors
    ├── ids.rs                        # QirId, PhysId, ExprId newtypes
    ├── expr.rs                       # side-effect-free scalar QExpr
    │
    ├── logical/
    │   ├── mod.rs                    # re-exports + LogicalPlan wrapper
    │   ├── operator.rs               # logical operator enum
    │   ├── lower.rs                  # HIR -> logical QIR lowering
    │   ├── shape.rs                  # nested shape, correlation modes
    │   └── links.rs                  # links-to-correlate lowering
    │
    ├── rewrite/
    │   ├── mod.rs                    # rewrite driver / fixpoint loop
    │   ├── merge_maps.rs             # merge adjacent Map operators
    │   ├── push_filter.rs            # push filters into scans/correlates
    │   ├── unnest_subqueries.rs      # correlated subquery -> joins/group-joins
    │   ├── decorrelate.rs            # apply/unnest/lateral rewrite core
    │   ├── lift_links.rs             # merge redundant links traversals
    │   └── flatten_project.rs        # Map-over-FlatMap fusion
    │
    ├── physical/
    │   ├── mod.rs                    # re-exports + PhysicalPlan wrapper
    │   ├── operator.rs               # physical operator enum
    │   ├── properties.rs             # ordering, distribution, streaming unit
    │   ├── planner.rs                # logical -> physical mapping
    │   ├── exchanges.rs              # exchange insertion
    │   ├── joins.rs                  # join algorithm selection
    │   └── aggregations.rs           # aggregate implementation selection
    │
    ├── exec/
    │   ├── mod.rs                    # execution plan contract
    │   ├── interface.rs              # backend trait / capability model
    │   ├── memory.rs                 # in-memory VM interpreter stub
    │   └── kernels.rs                # scalar kernel registry
    │
    ├── backend/
    │   ├── mod.rs
    │   ├── capability.rs             # BackendCapability + cost hints
    │   ├── memory_backend.rs         # in-memory array backend
    │   └── remote_backend.rs         # distributed/storage backend placeholder
    │
    ├── util/
    │   ├── arena.rs                  # arena helpers over yelang-arena
    │   ├── print.rs                  # debug/pretty printing
    │   └── graph.rs                  # DAG utilities
    │
    └── tests/
        ├── lower.rs                  # lowering unit tests
        ├── rewrite.rs                # rewrite unit tests
        ├── plan.rs                   # physical-plan unit tests
        └── integration.rs            # end-to-end VM stub tests
```

## Edge endpoint fields

`links` traverses edge tables. An edge struct is marked with `@edge` and must
declare endpoint fields. The canonical names are `_from` and `_to` to avoid the
keyword `from` and to match the record-id convention (`TableName:Value`).

```yed
@edge
struct Follows {
    _from: RecordId<User>,
    _to: RecordId<User>,
    since: i64,
}
```

- `_from` is the source endpoint; `_to` is the target endpoint for forward `->`.
- `<-` matches `_to == parent.id` and returns `_from`.
- `<->` matches either endpoint and returns the other endpoint.
- The type checker verifies that `_from` and `_to` are `RecordId<T>` for the
  table types declared in the path.

A future `@edge(from="src", to="dst")` form can allow renamed endpoints once
 the canonical form is stable.

## QIR logical model

### Core values

A QIR value is one of:

- `Value::Scalar(QExpr)` — a single value.
- `Value::Collection(QExpr)` — an ordered array/collection.
- `Value::Group { key, members }` — a grouping object.

`QExpr` is a side-effect-free expression over QIR values, similar to a
restricted HIR expression but with no local scopes or mutation.

### Operators

Every operator has a unique `QirId` and a list of child operator IDs.

```rust
pub enum Operator {
    /// Scan a named collection or an in-memory array value.
    Scan { source: ScanSource, item_ty: TyId },

    /// Filter a collection by a predicate over its element.
    Filter { input: QirId, predicate: QExpr },

    /// Map each element to a new value.
    Map { input: QirId, projection: QExpr },

    /// flat_map one or more levels.
    FlatMap { input: QirId, levels: u32, projection: QExpr },

    /// Order a collection.
    OrderBy { input: QirId, keys: Vec<OrderKey> },

    /// Slice/range a collection.
    Range { input: QirId, start: Option<QExpr>, end: Option<QExpr>, inclusive: bool },

    /// Join operators.
    InnerJoin { left: QirId, right: QirId, predicate: QExpr },
    LeftOuterJoin { left: QirId, right: QirId, predicate: QExpr },
    SemiJoin { left: QirId, right: QirId, predicate: QExpr },
    AntiJoin { left: QirId, right: QirId, predicate: QExpr },
    MarkJoin { left: QirId, right: QirId, predicate: QExpr, marker: Symbol },
    CrossJoin { left: QirId, right: QirId },

    /// Dependent join used as an intermediate during decorrelation.
    DependentJoin { outer: QirId, inner: QirId, predicate: QExpr },

    /// Group an input collection by key expressions.
    GroupBy { input: QirId, keys: Vec<(Symbol, QExpr)>, members_label: Symbol },

    /// Reduce a collection to a scalar.
    Aggregate { input: QirId, kind: AggregateKind },

    /// Window functions (row_number, rank, dense_rank, enumerate).
    Window { input: QirId, kind: WindowKind, partition: Vec<QExpr>, order: Vec<OrderKey> },

    /// Set operations.
    SetOp { op: SetOpKind, left: QirId, right: QirId },

    /// Distinct rows.
    Distinct { input: QirId, by: Option<Vec<QExpr>> },

    /// Attach a nested field to each upstream element.
    AttachField { input: QirId, field: Symbol, value_plan: QirId },

    /// Combine independent plans into a struct/object result.
    Construct { kind: ConstructKind, fields: Vec<(Symbol, QirId)> },

    /// Return a scalar value produced by a sub-expression.
    Expr(QExpr),
}
```

### Correlation and nested materialization

`links` lowers to joins plus `AttachField`. For a single segment:

```text
users
  LeftOuterJoin(follows on users.id == follows._from)
  AttachField "follows" => array of edges per user
    for each edge:
      LeftOuterJoin(friends on friends.id == edge._to)
      AttachField "friends" => array of friends per edge
```

`AttachField` materializes the nested array at the syntactic level where the
label is written, matching the Phase H semantics.

## Lowering from typed HIR to QIR

### Entry point

```rust
pub fn lower_query(tcx: &TyCtxt, body_id: BodyId, query_id: QueryId)
    -> Result<QirPlan, LoweringError>
```

The driver:

1. Looks up the typed HIR `Query` and resolved types from `TypeckResults`.
2. Builds a `QirPlan` (arena of operators keyed by `QirId`).
3. Lowers the pipeline into a DAG.
4. Lowers the projection expression into a `QExpr` attached to the final
   operator.

### `select ... from ...`

1. Lower each `FromNode` to a `Scan`.
2. Apply per-root modifiers as `Filter`/`OrderBy`/`Range`.
3. Single root: pipeline input is that root.
4. Multi-root: wrap each root sub-plan in `Construct { Facet, fields }`.

### `links`

Each `links` path lowers to a sequence of `LeftOuterJoin` + `AttachField`
rooted at the anchor. Direction operators determine which endpoint field is
used. Filters become join predicates or pre-filter operators.

### Array selectors / comprehensions

- `[*]` -> `Map`.
- `[where p]` -> `Filter`.
- `[**]` -> `FlatMap { levels: 1, projection: identity }`.
- chained suffix -> single `Map` whose projection is the suffix expression.

### `group by`

```yed
group by { city: u.city } into groups
```

lowers to `GroupBy { input, keys: [("city", u.city)], members_label: "users" }`.

## Logical rewrites

A fixed-point rewrite driver applies sound transformations:

- **Merge adjacent maps**: `Map(f) -> Map(g)` => `Map(f ∘ g)`.
- **Push filter into scan**: when the predicate references only element fields.
- **Push filter into join/correlate**: when the predicate is local to the join.
- **Flatten before project**: `Map(p) -> FlatMap(levels, identity)` =>
  `FlatMap(levels, p)`.
- **Decorrelate dependent joins**: transform `DependentJoin` into hash/merge
  joins, semi/anti joins, or group-joins based on the correlation predicate.
- **Lift redundant links**: merge sibling paths that share a prefix.

### Decorrelation

The core algorithm follows the standard relational unnesting recipe
(Dayal, Kim, Neumann):

1. Detect correlated sub-plans by finding free variables that reference outer
   operator outputs.
2. Represent them as `DependentJoin`.
3. Rewrite:
   - equality correlation -> `InnerJoin` / `LeftOuterJoin`
   - existential predicate -> `SemiJoin`
   - universal predicate -> `AntiJoin`
   - scalar aggregate with equality correlation -> `GroupJoin`
4. Fall back to `NestedLoopJoin` + broadcast for correlations that cannot be
   decorrelated.

References:
- U. Dayal, *Of Nests and Trees: A Unified Approach to Processing Queries that
  Contain Nested Subqueries, Aggregates and Quantifiers* (VLDB 1987)
- T. Neumann, *Improving Unnesting of Complex Queries* (BTW 2025)
- W. Kim, *On Optimizing an SQL-Like Nested Query* (TODS 1982)

## Physical planning

Physical operators mirror logical ones but add execution properties.

```rust
pub enum PhysOperator {
    TableScan { source: ScanSource, predicate: Option<QExpr> },
    Filter { input: PhysId, predicate: QExpr },
    Map { input: PhysId, projection: QExpr },
    FlatMap { input: PhysId, levels: u32, projection: QExpr },
    Sort { input: PhysId, keys: Vec<OrderKey> },
    Slice { input: PhysId, start: usize, end: Option<usize> },
    HashJoin { build: PhysId, probe: PhysId, build_key: QExpr, probe_key: QExpr },
    MergeJoin { left: PhysId, right: PhysId, keys: Vec<(QExpr, QExpr)> },
    NestedLoopJoin { outer: PhysId, inner: PhysId, predicate: QExpr },
    GroupJoin { outer: PhysId, inner: PhysId, key: (QExpr, QExpr), aggregate: AggregateKind },
    HashGroupBy { input: PhysId, keys: Vec<OrderKey>, members_label: Symbol },
    SortGroupBy { input: PhysId, keys: Vec<OrderKey>, members_label: Symbol },
    Aggregate { input: PhysId, kind: AggregateKind },
    Window { input: PhysId, kind: WindowKind, partition: Vec<QExpr>, order: Vec<OrderKey> },
    SetOp { op: SetOpKind, left: PhysId, right: PhysId },
    Distinct { input: PhysId, by: Option<Vec<QExpr>> },
    AttachField { input: PhysId, field: Symbol, value_plan: PhysId },
    Construct { kind: ConstructKind, fields: Vec<(Symbol, PhysId)> },
    Exchange { input: PhysId, kind: ExchangeKind },
    Gather { inputs: Vec<PhysId> },
    Expr(QExpr),
}
```

### Properties

Each operator carries:

- **Ordering**: ordered by expressions, or `Arbitrary`.
- **Distribution**: `Single`, `Broadcast`, `Hash(Symbol)`, `Replicate`,
  `Unknown`.
- **Streaming unit**: `PerParent`, `Leaf`, `Scalar`.

### Exchange insertion

Insert `Exchange` when a parent requires a distribution the child does not
satisfy:

- `HashGroupBy` -> `RepartitionBy(keys)` if not already keyed.
- `HashJoin` build side -> `Broadcast` or `RepartitionBy(build_key)`.
- Global `Sort` -> `Gather` + `Sort`.

In single-node mode `Exchange` is a no-op planning boundary.

## Backend capability model

QIR operators are backend-agnostic. Physical planning consults a
`BackendCapability` trait:

```rust
pub trait BackendCapability {
    fn can_push_down_filter(&self, source: &ScanSource) -> bool;
    fn can_push_down_order(&self, source: &ScanSource) -> bool;
    fn can_push_down_limit(&self, source: &ScanSource) -> bool;
    fn supports_index_lookup(&self, source: &ScanSource, key: &[Field]) -> bool;
    fn supports_hash_join(&self) -> bool;
    fn supports_merge_join(&self) -> bool;
    fn supports_exchange(&self, kind: ExchangeKind) -> bool;
    fn supports_aggregation(&self, kind: AggregateKind) -> bool;
    fn estimated_cardinality(&self, source: &ScanSource) -> Cardinality;
}
```

Phase I provides:

- `MemoryBackend`: supports everything locally.
- `RemoteBackend` (placeholder): limited push-down, requires explicit exchanges.

## Execution interface

```rust
pub trait QueryExecutor {
    type Error;
    fn execute(&self, plan: &PhysicalPlan) -> Result<Value, Self::Error>;
}
```

Phase I provides an in-memory interpreter in `exec::memory` for testing.

## Collection methods and native recognition

QIR does not special-case syntax. Standard-library methods lower to QIR
operators only when the callee resolves to a known `DefId` or lang item and
all legality conditions hold (determinism, effect-freeness, preserved errors
and cardinality, backend capability).

| Surface | Logical operator |
|---|---|
| `.map`, `@x[*].expr` | `Map` |
| `.filter`, `[where p]` | `Filter` |
| `.flat_map`, `.flatten`, `[**]` | `FlatMap` |
| `.order_by`, `[order by]` | `OrderBy` |
| `.slice`, `.range`, `[a..b]` | `Range` / `LimitOffset` |
| `.group_by`, pipeline `group by` | `GroupBy` |
| `.index_by`, `.key_by`, `.associate_by` | `LookupBuild` |
| `.any`, `.all`, `.none` | `SemiJoin`, `AntiJoin`, `MarkJoin` |
| `len`, `count`, `.count()` | `Aggregate(Count)` |
| `sum`, `min`, `max`, `avg` | `Aggregate(...)` |
| `.rank_by`, `.enumerate()` | `Window` |
| `.distinct`, `[distinct]` | `Distinct` |
| `.union`, `.intersect`, `.except` | `SetOp` |
| `.join_by`, `.left_join_by`, `.semi_join_by`, `.anti_join_by` | joins |
| `links ...` | `Traverse` / joins + `AttachField` |
| `graph::transitive_closure` | `RecursiveExpand` / `Fixpoint` |

## Diagnostics

Lowering and planning can emit:

- `LoweringError::UnsupportedSelector` for selector forms not yet lowered.
- `LoweringError::NonLiteralRange` for unsupported dynamic range bounds.
- `LoweringError::AmbiguousGroupTarget` for multi-root `group by`.
- `LoweringError::InvalidEdgeEndpoints` for missing `_from`/`_to`.

## Test strategy

### Lowering tests

One test per construct verifying operator shape:

- scalar projection,
- map/filter/order/range,
- nested selectors with `[**]`,
- `links` forward/backward/bidirectional,
- multi-hop and continuation,
- `group by`,
- multi-root `from`,
- mutation queries.

### Rewrite tests

Before/after plan graphs for each rewrite rule.

### Physical-plan tests

- join algorithm selection,
- exchange insertion,
- map fusion,
- filter push-down.

### Integration tests

Parse + type-check + lower + plan + execute against the in-memory VM and assert
result value. Cover all Phase H integration scenarios.

## Deferred to later phases

- Cascades-style cost-based optimizer.
- Real distributed execution and exchange implementations.
- Streaming / async pull-based execution.
- Index-aware table scans and cardinality estimation.
- Query caching and prepared-query plans.
- Variable-depth / recursive graph traversal in `links`.

## References

- `PHASEH_QUERY_EXPRESSION_DESIGN.md`
- `notes/syntax_grammar/select.md`
- `notes/syntax_grammar/semantics.md`
- `notes/syntax_grammar/pipelining.md`
- `notes/syntax_grammar/nested-array-navigation.md`
- `notes/syntax_grammar/name-resolution-and-scoping.md`
- `notes/syntax_grammar/native-collection-query-surface.md`
- `notes/syntax_grammar/standard-library-data-structures-and-algorithms.md`
- U. Dayal, *Of Nests and Trees* (VLDB 1987)
- T. Neumann, *Improving Unnesting of Complex Queries* (BTW 2025)
- W. Kim, *On Optimizing an SQL-Like Nested Query* (TODS 1982)
- G. Graefe, *Volcano — An Extensible and Parallel Query Evaluation System*
