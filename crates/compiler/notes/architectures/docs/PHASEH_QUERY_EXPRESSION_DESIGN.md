# Phase H: Query Expressions and Array Selectors

This document describes the design, lowering, and type-checking of single-root
`select ... from ... where ... order by ... range ...` query expressions and
binder-bearing array selectors (`[*]`, `[where ...]`, `[**]`).

## Status

Implemented:
- Single-root and multi-root `select` with scalar or array projection.
- `from` source modifiers (`where`, `order by`, `range`) and top-level tail
  clauses (`where`, `order by`, `range`) for single-root queries.
- Multi-root `from` with per-root `for <root> { ... }` post-`links` tail
  disambiguation blocks.
- Array selectors `[*]` (map), `[where ...]` (filter), and `[**]` (flatten) on
  paths and local array values, including chained selector field/method access
  and combined filter+flatten chains (`base@b[where ...][**]`).
- `links` graph traversal with forward (`->`), backward (`<-`), and
  bidirectional (`<->`) edge hops, multi-hop paths, continuation from
  intermediate labels, and traversal-local filters.
- Virtual nested fields created by `links` are typed and accessible in
  projection, including field access on edge and target element types.
- `group by` with single/composite keys and `into <label>` result binding.
- Auto-call of zero-arg function items used as selector/query sources.
- Array literals producing the dynamic prelude `Array<T>` type and fixed-size
  array types (`[T; N]`, `[value; N]`).
- Built-in array predicates `len`, `count`, `is_empty`, `any`, `all`.
- Mutation queries (`create`, `update`, `upsert`, `delete`, `link`, `unlink`)
  with object payloads, `set`/`merge`, `where`, and `; <expr>` tail clauses.
- `_` return-type inference for query and array expressions.

Deferred to later phases:
- Lowering typed query expressions to a dedicated Query IR (QIR) / physical plan.
- Selector-local `order by`, slicing `[n..m]`, `distinct`, `enumerate`.
- Closure signature checking for `any` / `all` predicates.
- Node-to-node `links` hops without an edge segment.
- Union edge types in a single segment (`actions@e:(A | B)`).
- Variable-length / recursive traversals and fork blocks.
- Dedicated `distinct` / `unique_by` pipeline operators.

## Goals

1. Keep query expressions first-class expressions while requiring an explicit
   `from` clause (no rootless `select`).
2. Preserve the "projection defines the result" rule: the result type of a
   query is exactly the type of the projection expression; pipeline clauses only
   determine names, collections, and filtering.
3. Represent selectors uniformly as `Expr::Comprehension` so that later phases
   (QIR lowering, optimization) see a single, well-typed iteration primitive.
4. Reuse the prelude `Array<T>` lang item as the canonical dynamic collection
   type for query results and array literals.

## File tree

```text
crates/compiler/
├── yelang-hir/src/
│   ├── hir/query.rs       # Query, SelectQuery, mutation query structs
│   ├── hir/expr.rs        # Expr::Query, Expr::Comprehension
│   └── lowering/expr.rs   # lowers select, selectors, auto-calls fn sources
├── yelang-tycheck/src/
│   ├── check.rs           # check_query, check_comprehension, check_array
│   ├── fn_ctxt.rs         # mk_array_ty, expect_array
│   └── array_builtins.rs  # len, count, is_empty, any, all intercepts
└── yelang-tycheck/tests/integration.rs
    # end-to-end query/selector tests
```

## HIR representation

### `Expr::Query`

A `select` query is lowered to `Expr::Query(QueryId)`, where the arena owns the
`Query` node.

```rust
pub struct SelectQuery {
    pub projection: ExprId,
    pub from: Vec<FromNode>,
    pub links: Vec<SelectLinkPath>,
    pub post_links_for: Vec<ForRootModifiers>,
    pub where_clause: Option<ExprId>,
    pub order_by: Vec<OrderByPart>,
    pub range: Option<QueryRange>,
    pub group_by: Option<GroupByClause>,
}

pub struct FromNode {
    pub source: ExprId,
    pub label: Symbol,
    pub binder: PatId,
    pub elem_ty: Option<HirTyId>,
    pub filter: Option<ExprId>,
    pub order_by: Vec<OrderByPart>,
    pub range: Option<QueryRange>,
}

pub struct SelectLinkPath {
    pub start: SelectLinkNode,
    pub segments: Vec<SelectLinkSegment>,
}

pub struct SelectLinkSegment {
    pub direction: EdgeDirection,
    pub edge: SelectLinkEdge,
    pub target: SelectLinkNode,
}

pub struct GroupByClause {
    pub keys: Vec<GroupByKey>,
    pub into_binder: PatId,
}
```

- `from` may contain multiple roots; multi-root queries require per-root
  `for <root> { ... }` blocks instead of global `where`/`order`/`range`.
- `links` paths introduce virtual nested arrays that are typed and reachable
  from upstream element binders or element types.
- `group_by` transforms the root stream into a collection of group objects
  with `key` and `members` fields.
- The `from` source expression is auto-called when it is a path to a function
  item, so `from users@u:User` works whether `users` is a value of type
  `Array<User>` or a function `fn users() -> Array<User>`.

### `Expr::Comprehension`

Array selectors desugar to `Expr::Comprehension`:

```rust
pub struct ComprehensionVar {
    pub pat: PatId,
    pub source: ExprId,
    pub flatten: u32,
}

pub enum ComprehensionKind {
    List,
    Set,
    Dict,
}

pub enum Expr {
    // ...
    Comprehension {
        kind: ComprehensionKind,
        element: ExprId,
        variables: Vec<ComprehensionVar>,
        condition: Option<ExprId>,
    },
}
```

- `[*]` becomes `flatten: 0`.
- `[**]` becomes `flatten: 1` (flattens one level).
- `[where e]` becomes `condition: Some(e)` and `flatten: 0`.
- Selector chains such as `users@u[*].address.city` produce a single
  comprehension whose `element` is the suffix expression `u.address.city`.

## Lowering desugaring

### `select <projection> from <source>@<binder>:<Ty> ...`

1. Lower `<source>` as an expression.
2. If the lowered expression is a path to a function item, wrap it in a
   zero-argument call (`auto_call_fn_source`).
3. Allocate a binding pattern for `<binder>`.
4. Lower `<projection>` in a scope containing the binder.
5. Build `SelectQuery { projection, from: [FromNode { source, binder, ... }], ... }`.

### `expr@binder[selector].suffix`

The selector base is peeled by `lower_with_selector_base`, which walks down
member-access and method-call chains recursively until it finds the leftmost
binder-bearing selector. The entire accumulated suffix is then built inside the
selector's comprehension scope, so intermediate array results never appear as
plain expressions:

```yed
users@u[*].address.city
```

lowers to a single comprehension whose element is `u.address.city` and whose
type is `Array<i32>`. Method-call arguments in a chain such as
`users@u[*].foo(arg)` are lowered in the enclosing scope before the selector
scope is entered, matching the evaluation order of a comprehension element.

### Array literals

`[e1, e2, ...]` lowers to `Expr::Array { exprs: [...] }`. Type checking then
produces `Array<T>` where `T` is the unified element type, using the prelude
`Array<T>` lang item when available.

### `Array<T>` lang item

Because `Array` lives in the prelude and has no user-written source definition,
`LoweringContext::synthesize_array_item` injects a minimal generic HIR struct:

```rust
struct Array<T> {
    _phantom: T,
}
```

This gives the type collector real `AdtDefData` and `GenericsData` for the
lang item, which is required for `check_array` and `mk_array_ty` to build
instantiated `Array<T>` types.

### `[T]` as `Array<T>`

The surface syntax `[T]` is the canonical spelling for the dynamic array type
`Array<T>`. It is lowered to a resolved path type `Array<T>` using the prelude
`Array` lang item. Fixed-size arrays keep the distinct `[T; N]` form and lower
to `Ty::Array(T, N)`.

### Mutation query tails

Mutation queries (`create`, `update`, `upsert`, `delete`, `link`, `unlink`) do
not use a `return` clause; `return` is reserved for function-level early exits.
In block form, the value produced by the mutation is introduced by `; <expr>`:

```yed
create {
  user@u:User { id: 1, name: 'Alice' }
  link (u)->[follows]->(friend)
  ; u.name
}
```

If the tail expression is omitted, the mutation expression evaluates to `()`.

## Type-checking rules

### `select` query

For each `FromNode`:

1. `source` must have an array type. If `elem_ty` is annotated, demand
   `source: Array<elem_ty>`; otherwise `expect_array(source)` infers the element
   type.
2. The binder pattern is checked against the element type.
3. `filter` must be `bool`.
4. `order_by` expressions are checked for well-formedness (no type constraint
   yet).
5. `range` bounds are checked as expressions (expected integer; enforcement is
   partial).

Top-level `where_clause` must be `bool`. Top-level `order_by` and `range` are
checked similarly.

The result type of the query is the type of `projection`.

### Comprehension / selector

For each `ComprehensionVar`:

1. Check the `source` expression.
2. `expect_array(source)` yields the element type.
3. Apply `flatten` additional `expect_array` steps to obtain the iterated
   element type.
4. Check the binder pattern against the iterated element type.

If `condition` is present, it must be `bool`.

For `ComprehensionKind::List` the result type is `Array<T>` where `T` is the
element expression type.

### Array literals

`[e1, ..., en]` (including `[]`):

1. All elements must have the same type (or introduce a fresh type variable for
   an empty literal).
2. If the `Array` lang item is present, the result is `Array<T>`.
3. If the lang item is absent (isolated unit tests without prelude), fall back
   to the fixed-size `Ty::Array(T, n)`.

### Array builtins

- `len(xs)` / `count(xs)`: `xs` must be an array; result `usize`.
- `xs.is_empty()`: `xs` must be an array; result `bool`.
- `xs.any(p)` / `xs.all(p)`: `xs` must be an array, `p` must be callable; result
  `bool`. The closure parameter type is not yet verified against the element
  type.

## Coercion and inference

`Array<T>` uses ordinary ADT unification. `[]` produces `Array<?T>`; coercion to
an annotated return type such as `Array<User>` unifies `?T` with `User`.

`_` return types are inferred from the body, so object projections in `select`
can return anonymous struct types without an explicit annotation.

## Diagnostics

- `where` / selector filter not `bool`: `type mismatch`.
- Source not an array: "expected an array type, found `...`".
- Field access on an array: "no field `...` on type `Array<...>`".
- `len` / `count` / `is_empty` on non-array: "expected an array type, found `...`".
- Multi-root `from`, `links`, `group by`, `for <root>`: lowering error
  (`UnsupportedAst`) reported during HIR construction.

## Exhaustive test coverage

The integration tests in `yelang-tycheck/tests/integration.rs` cover:

- Scalar and array projections from `select`.
- `from` source modifiers (`where`, `order by`, `range`).
- Top-level tail clauses (`where`, `order by`, `range`).
- Object projections in `select`.
- Nested field access through selectors (`users@u[*].address.city`).
- Selector filters combined with nested field access
  (`users@u[where u.age > 18].address.city`).
- Chained selectors (`map` then `filter`, `flatten` then `map`).
- Flattening with `[**]`.
- Fixed-size array literals and `[value; N]` repeat expressions.
- Dynamic array type `[T]` lowered to `Array<T>`.
- `_` return-type inference driven by query/array results.
- Mutation query tails using `; <expr>` instead of `return`.
- `len`, `count`, `is_empty`, `any`, `all`.
- Mutation queries (`create`, `update`, `upsert`, `delete`, `link`, `unlink`)
  with object payloads, `set`/`merge`, `where`, and `; <expr>` tail clauses.
- Error cases: non-array `len`, non-array `is_empty`, field access on array,
  selector filter not `bool`, query `where` not `bool`, fixed-size array length
  mismatch, create/update field type mismatch.

## Deferred features

| Feature | Why deferred | Current behavior |
|---------|--------------|------------------|
| `links` graph traversal | Requires edge/table semantics, materialized nested fields, and correlation model. | Rejected during lowering. |
| `group by` | Requires grouping keys, aggregate scoping, and `into <label>` semantics. | Rejected during lowering. |
| Multi-root `from` | Requires per-root tail clauses and independent facet semantics. | Rejected during lowering. |
| Selector `order by`, slices, `group by`, `distinct`, `enumerate` | Need lowering from array-index forms to comprehensions or QIR. | Lowering error. |
| `any`/`all` closure signature checking | Closure signatures are not yet fully lowered/typed. | Accepted with any single argument. |

## References

- `notes/syntax_grammar/select.md`
- `notes/syntax_grammar/semantics.md`
- `notes/syntax_grammar/name-resolution-and-scoping.md`
- `notes/syntax_grammar/complex_query.sql`
