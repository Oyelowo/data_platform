# Semantics (proposal)

## Status: spec vs implementation

This document describes the intended semantics.


Implementation note:
- The optimizer/physical planner may insert internal `Exchange` barriers (broadcast/shuffle/gather/merge) to model distribution boundaries.
- In the single-node executor this is semantics-preserving and acts as a planning boundary.
- When executing with the sharded/mock-distributed table source, the VM interprets `Exchange` as a real distribution barrier and can run joins shard-locally when inputs are compatible.

This file captures a proposed mental model for path expressions, array selectors, bindings (`@u`), and how `from`/`links` feed data into `select`.

**Canonical scoping rules:** for name resolution, binder identity, and `links` scoping/uniqueness rules, see:
- `src/syntax/name-resolution-and-scoping.md`

The goal is: you can treat collections/arrays as first-class values, still be able to talk about both “the whole array” and “the current element”, and avoid falling back to a separate DSL for common questions like `len(users)`.

## 0.1) The single most important rule: projection defines the result

In this language, `select <projection>` is an *expression*.

But it is not a rootless expression form.

- Surface `select` requires an explicit `from` clause.
- `from` is where the source collections and query binders come from.
- `select 1` without `from` is not part of YeLang semantics.

- The **pipeline** (`from/links/where/group/order/range`) determines what *names* exist and what *collections* those names denote.
- The **projection** determines the *result value and shape* of the query.

There is **no implicit row-by-row mapping** from `from` into `select`.
If you want a per-element result, you must express it with selectors like `[*]`, `[where ...]`, `[n]`, slices, or `[**]`.

Examples:

```yed
// Scalar result: projection is a scalar, independent of how many users exist.
select 1
from users@u:User

// Array result: explicit iteration with `[*]`.
select users@u[*].id
from users@u:User

// Scalar result by navigation: index into the materialized root collection.
select users[0].id
from users@u:User
```

`select 1 from users@u:User` returns `1`, not `[1, 1, ...]`.

Optional chaining is still just projection semantics:

```yed
select users@u[*].best_friend?.id
from users@u:User
```

`?` stays an option-like chain on the projected value. It is not SQL-style null-row rewriting, and follow-on consumers like `is_none()` / `is_some_and(...)` remain ordinary option-like method calls on that projected result.

### RANGE does not change types

`range` constrains *collections in the pipeline*; it does not “scalarize” results.
Whether the result is an array or a scalar is still determined solely by the projection.

```yed
// Still an array, just length <= 1.
select users@u[*].id
from users@u:User
range ..1

// Still a scalar (indexing makes it a scalar), and the range only affects what `users` contains.
select users[0].id
from users@u:User
range ..1
```

Grouping keys also do not override the projection result.

```yed
// Still returns `1`.
select 1
from users@u:User
group by {
  latest_post_id: (select posts@p[*].id
      from posts@p:Post
      where p.user_id == u.id
      range ..1)[0],
} into groups
```

The grouped pipeline above changes which names and collections exist (`groups`, group keys, grouped members),
but the result is still the projection expression `1`.

Equivalent scalar subquery spellings are allowed when they preserve the same projection semantics:

```yed
// Project the array first, then index the result.
(select posts@p[*].id
    from posts@p:Post
    where p.user_id == u.id
    range ..1)[0]

// Project the indexed scalar directly from the ranged source collection.
select posts@p[0].id
from posts@p:Post
where p.user_id == u.id
range ..1
```

Those two forms are equivalent because:

- `range` constrains the source collection seen by the query
- the projection determines whether the nested query returns an array or a scalar
- `[0]` remains ordinary projection-level scalarization over that ranged collection

As usual, if determinism matters, make the path order explicit before indexing.

## 1) Two kinds of values: **one** vs **many**

- **one**: a single scalar/object (e.g. `User`, `Int`, `{...}`)
- **many**: an array/collection (e.g. `Array<User>`)

Most of the language can be understood as “operators that keep you in one” vs “operators that move you into many”.

## 2) Member access and projection

- `x.field`: member access on a **one** value.
- `x.{ a, b, renamed: expr }`: object projection on a **one** value.

Key principle (no magic): **`.` only works on a single object**.

If you have a **many** value (`Array<T>`), you must first enter an element context (map / flat_map) before you can do `.` field access or object projection:

- `[*]` changes the current value from `Array<T>` to `T` (repeated over all elements).

Rule: `array.field` and `array.{...}` are invalid; use `array@t[*].field` / `array@t[*].{...}`.

Mapping forms:

- `array[*].field` returns `Array<FieldType>`
- `nested[**].field` returns `Array<FieldType>` (flat_map one level, then access the field)
- `[where ...]` keeps the current value as `Array<T>` but filters which elements remain.

If your current value is a *nested* array (`Array<Array<T>>`), you must flat_map before you can access fields on elements:

- `[**]` changes the current value from `Array<Array<T>>` to `T` (repeated over all inner elements).

More generally: if there are $n$ `*` characters inside the brackets, the flat_map depth is $n - 1$.

Example pattern (explicit binders follow the array selectors):

```yed
users@u[*].writes@w[**].books@b[**].{ ... }
```

### Common pitfall: nested arrays require flattening *at the level you access fields*

When you chain multiple `[*]` selectors, you often end up with a nested array value.

Example (conceptual shape):

```text
u.befriends@e[*].friends@f[*] : Array<Array<User>>  (aka [[User]])
```

At that point:

- `u.befriends@e[*].friends@f[*].id` is **invalid**, because `.id` would be applied to the
    *current value*, which is `[[User]]` (a nested array), not a single `User`.

This is enforced by the typechecker: field/document access on arrays requires explicit mapping, and field access on an array of arrays is rejected.

To fix it you have two options depending on the shape you want:

1) **Flatten to a single array** (get `[string]`):

```yed
u.befriends@e[*].friends@f[**].id
```

2) **Keep the nested shape** (get `[[...]]`) by mapping at the appropriate level:

```yed
// Produces [[{ id: string }]]
u.befriends@e[*].{ friends: e.friends@f[*].{ id: f.id } }
```

If you specifically want `[[string]]`, one explicit way is:

```yed
u.befriends@e[*].{ ids: e.friends@f[*].id }[*].ids
```

Deep chained traversals follow the same rule.

If you keep traversing through multiple link-attached collections and want to continue field access at
each stage, flatten at the stage that would otherwise leave the current value as a nested array.

Example:

```yed
// Flat continuation shape.
u.writes@w[where w.date > dt'2024-02-01'][order by w.date asc]
    .books@b[**]
    .read_by@r[**]
    .readers@ru[**]
    .follows@f[**]
    .other_users@ou[**]
    .id
```

By contrast, this introduces an extra nesting level at `read_by`:

```yed
u.writes@w[where w.date > dt'2024-02-01'][order by w.date asc]
    .books@b[**]
    .read_by@r[*]
```

At that point the current value is nested, so continuing directly with `.readers...` is invalid unless
you intentionally preserve that nested shape and remap it explicitly.

## 3) Array selectors (the bracket operators)

All array selectors live in `[...]`. The operand must be a **many** value.

### 3.1 `[*]` — map/iterate

`users@u[*]` means “iterate all elements of `users` and bind each element to `u` for the remainder of the path”.

Conceptually:

- `users@u[*].expr` ~ `users.map(|u| expr)`

Examples:

```py
users@u[*].age
users@u[*].{ id, age }
```

### 3.2 `[where ...]` — filter

`users@u[where u.age > 5]` is a filter (not a map):

- `users@u[where pred(u)]` ~ `users.filter(|u| pred(u))`

Examples:

```py
users@u[where u.age > 5]
users@u[where u.nick_name == "bob"]
```

### 3.3 `[n]` — index

`users[0]` selects one element.

For plain arrays, and for query/path collections, out-of-bounds is a **runtime error** (no implicit `null` / `Option<T>` at the language surface).

For query/path collections, indexing is also **order-sensitive**.

If the path region has explicit order, `[n]` means the nth element under that order.
If the path region has no explicit order, `[n]` is still valid but uses an arbitrary engine-chosen order.
That means the result is intentionally nondeterministic across lawful plan changes unless you add `[order by ...]`.

Current semantic-lowering rule:

```text
selector indexing always lowers
`[order by ...][n]` records explicit path order
bare `[n]` records arbitrary selector order
```

Reason:

```text
ordered `[n]` means "the nth element under a defined order"
bare `[n]` means "the nth element under an arbitrary engine-chosen order"
```

### 3.4 `[start..end]` — slice/range

Slice syntax uses `..` / `..=`:

- `users[0..3]` (end-exclusive)
- `users[0..=2]` (end-inclusive)
- `users[..3]`, `users[3..]`

The important part is: slicing returns a **many** value.

For query/path collections, slices follow the same order rule as indexing:

```text
[order by ...][0..3] -> deterministic slice under explicit order
[0..3]               -> arbitrary slice under engine-chosen order
```

### 3.5 `[**]` / `[***]` / ... — flat_map (multiple depths)

`[**]` is the flat_map accessor: it flattens one level and enters element context.

Rule: if there are $n$ `*` characters inside the brackets, the flat_map depth is $n - 1$.

- `[**]` flat_maps one level: `Array<Array<T>> -> T` (repeated over all inner elements)
- in general, more `*` means more flat_map depth:
    - `[***]` flat_maps 2 levels
    - `[****]` flat_maps 3 levels

This makes nested traversals ergonomic:

```py
users@u[*].tags@t[*]    // Array<Array<Tag>>
users@u[*].tags@t[**]   // Array<Tag>
users@u[*].tags@t[****] // flatten 3 levels (if needed)
```

Conceptually:

- `users@u[*].tags@t[*]` ~ `users.map(|u| u.tags.map(|t| t))`
- `users@u[*].tags@t[**]` ~ `users.flat_map(|u| u.tags)`

Implementation note:
- When selector flattening (`[**]`, `[***]`, ...) appears in the final projection, QIR physical lowering upgrades the plan’s streaming hint to `PhysStreamUnit::Leaf` (this is a planning hint, not a semantic change).

## 3.6 Collection predicates: `any` / `all` / `is_empty`

These helpers are defined on **many** values (arrays/collections). Their meaning is purely
semantic; implementations are free to short-circuit and/or decorrelate them when it is
correct to do so.

- `xs.is_empty()` is `true` iff `len(xs) == 0`.
- `xs.any(|x| p(x))` is `true` iff there exists an element `x` in `xs` such that `p(x)` is true.
- `xs.all(|x| p(x))` is `true` iff for every element `x` in `xs`, `p(x)` is true.

Notes:
- Predicate closures run in a scope where both the closure parameter (the current element) and
    any outer bindings captured by the closure are in scope.
- The type of the closure parameter is the **element type of the collection**.
    - If the collection is `Array<Post>`, then `.any(|p| ...)` binds `p: Post`.
    - If the collection is `Array<i64>`, then `.any(|id| ...)` binds `id: i64`.
- Short-circuiting is semantically observable when the predicate can raise runtime errors
    (e.g. indexing out of bounds), so implementations must preserve the language meaning.

Implementation status note:
- `is_empty()` is VM-intercepted and executable.
- `any/all/none` are VM-intercepted and executable for arrays (including arrays materialized by
    `links`). In query contexts, the optimizer may also decorrelate some forms into join operators.

Examples:

```yed
// Elements are objects.
(select posts@p[*] from posts@p:Post).any(|p| p.id > 0)

// Elements are scalars.
(select posts@p[*].id from posts@p:Post).any(|id| id > 0)
```

## 4) Binding rules (`@u`)

Note: the full rules for binder identity (introduce-once in pipeline; reference-many; no rebinding) and the namespace behavior of comma-separated `links` paths are specified in:
- `src/syntax/name-resolution-and-scoping.md`

### 4.1 Where the binding is valid

When you write `expr@u[...]`, the binding `u` is in scope:

- inside `[where ...]`
- to the right of `[*]` / `[**]` in the rest of the path chain (e.g. `users@u[*].tags`)
- inside object projections like `users@u[*].{ ... }`

### 4.2 “whole array” vs “current element”

The label **without** `@` refers to the whole collection (a **many** value).

The binding after `@` refers to the current element (a **one** value).

This distinction is what enables:

```py
len(users)          // whole array
users@u[*].age      // per-element
```

## 5) `from` / `links`: scope + nested link results (proposal)

Note: this section describes the **data-shape model** of `links` (materializing nested fields). For the **name/scoping constraints** (anchors, uniqueness across multi-path links, and when a name is an introduction vs a reference), use:
- `src/syntax/name-resolution-and-scoping.md`

This is the key design choice you’re circling:

### Alternative (not chosen): join/tuple model

You can model `links` as producing a combined stream of tuples (join-like). That is powerful for some optimizers, but it makes path navigation feel less literal (e.g. `u.writes` isn’t a real “field”; it’s a tuple component).

### Chosen model (recommended): `links` *materializes nested fields*

`links` declares traversals that become *virtual nested arrays* reachable from upstream items.

Implementation note:
- Although the surface model is “nested materialization”, the compiler/runtime is free to implement a traversal either as a dedicated traversal operator or by compiling it into an explicit join-chain (joins + per-parent nest materialization) in later lowering phases.
- This is an execution strategy choice and must not change observable semantics (strict locality, ordering, multiplicity, and which runtime errors occur).

Under this option, if you do:

```py
from (users@uf:User)
links (users)->[writes@wl:UserWritesBook]->(books@b:Book)
```

Then within projection, you can use a “nested” access from the element:

```py
select users@u[*].{
    id,
    books_written: u.writes@w[*].books@b[**].{ title }
}
```

Suggested semantics:

- `users` is bound as a whole-array value (usable for `len(users)`, `count(users)`, slicing, etc.).
- `uf` is a per-element binding usable where a per-element binding makes sense (inline filters and link filters).
- `writes` and `books` are nested results associated with the relevant upstream element.
- Missing traversals yield **empty arrays**. If you need “exactly one”, use an explicit check (e.g. index `[0]` and accept that empty is a runtime error, or a dedicated helper like `expect_one(...)` if/when added).

### Name collisions

If a real schema field and a link traversal share the same label (e.g. `u.writes` exists as a real field), pick one of these rules:

- **(simple)** forbid collisions (error) unless user renames the traversal label
- **(powerful)** allow collisions but require an explicit disambiguator (new syntax)

The collision-forbidden rule is simplest and keeps `.field` unambiguous.

### 5.1 Link direction operators: `->` / `<-` / `<->`

Assume an edge row has two endpoint fields (currently modeled as `_from` and `_to`).

- `->` (forward): match edges where `_from == current.id`, and the "other endpoint" is `_to`.
- `<-` (backward): match edges where `_to == current.id`, and the "other endpoint" is `_from`.
- `<->` (bidirectional): match edges where `_from == current.id` **or** `_to == current.id`, and the "other endpoint" is whichever of `_from`/`_to` is not `current.id`.

Concrete mental model (per upstream element `u`):

```py
from users@u:User

// forward adjacency
links (users)->[edges@e:Edge]->(nodes@n:Node)

// backward adjacency
links (users)<-[edges@e:Edge]<-(nodes@n:Node)

// bidirectional adjacency (undirected traversal over a directed edge table)
links (users)<->[edges@e:Edge]<->(nodes@n:Node)
```

Important notes:

- Bidirectional traversal is most meaningful when the edge connects the same node type/table on both ends (e.g. User↔User friendship).
- If you store both directions as separate rows (A→B and B→A), `<->` will naturally see both unless the query explicitly deduplicates.
- `<->` is a query-time traversal meaning; it does not imply that LINK/UNLINK should create/delete both directions.

### 5.2 Multiple link paths, references, and extra filters

This section answers: “what if a subsequent path starts from a reference and adds extra filters?” and “what if a subsequent path starts from a declaration instead?”

#### Multiple paths (`,`)

If a `from`/`links` clause contains multiple link paths (comma-separated), they are interpreted as multiple traversal *branches* that contribute to a single logical result.

Pragmatically:

- Each individual path contributes nested arrays at the labels it mentions.
- Each path must start from an anchor that is already in scope from `from` / earlier pipeline clauses.
- Paths are interpreted **left-to-right**. Labels introduced by earlier sibling paths are in scope for later sibling paths, so a later path may anchor from an intermediate label introduced earlier in the same `links` clause.
    - Anchoring from a label introduced only by a later sibling path is a compile error.
- The overall result is the *merge* of all contributed nested fields (missing branches yield empty arrays).

#### Final verdict (spec decision)

This section is intentionally “decision grade”. It is the semantics that should be consistent, unsurprising, and implementable.

1) `from` declares roots

- `from` is the only construct that declares a root collection scan in a `select` query.
- Multi-root is expressed only by multiple `from` items (e.g. `from users@u:User, books@b:Book`).

Important: multi-root `from` does **not** imply an implicit cross-product or join stream. Cross-root relationships must be expressed explicitly (e.g. via `links`, or via a nested single-root `select` that makes the stream you want explicit). As a consequence, top-level tail clauses are intentionally rejected for multi-root queries, and per-root `for <root> { ... }` tail blocks are root-local (they cannot reference other roots’ item binders).

2) `links` is correlated traversal, not a general join

- `links` traversals run *per upstream element* of a base array and materialize nested arrays onto those elements.
- `links` does not create new independent roots and does not imply cross-products.

Materialization rule (strict locality):

- A traversal segment materializes fields **only at the syntactic level where they are written**.
- In particular, for an edge-table hop `(users)->[writes]->(books)`:
    - `writes` is materialized on the parent user elements.
    - `books` is materialized on the per-edge objects inside `writes`.
    - `books` is *not* automatically duplicated/hoisted onto the user object.

This avoids ambiguity when labels overlap with real schema fields and keeps “what you see” aligned with “what you wrote”.

3) Each `links` path starts from a bound reference

- The first node label in each `links` path is the base array.
- That first node must be a reference to an already-bound label (a `from` root label, or a label produced by an earlier traversal).
- If the first node label is not bound, the path is rejected (no implicit re-anchoring/rotation).

4) Filters are allowed anywhere and are always local

- Base-node filter: gates whether the traversal runs for a given parent element; it does not remove items from the base array.
- Later-node filter: gates reached nodes/edges for that segment.
- No filter retroactively removes elements created by earlier paths.

5) Labels name materialized arrays; binders do not

- Labels (`users`, `books`, `written_by`, `other_users`) determine output field names.
- `@binders` exist only for filtering and expression binding; they do not rename output keys.

Binding vs materialization:

- A node/edge label can still introduce a **collection binder** for continuation (starting a later `links` path) and for whole-collection operations.
- That binder does not imply there must be a same-named field materialized at every level.

#### Anchoring rule (where does a subsequent path attach?)

Anchoring must be deterministic and obvious, otherwise users will (reasonably) expect graph-equivalent patterns to behave the same.

For `select ... links ...`, each traversal path is **correlated** (it runs “per upstream element”), so it must have a single explicit *base array*.

Rule (for `links` inside `select`):

- The **first node** in each `links` path is the anchor/base.
- The first node must be a **reference** to an already-bound label (either the root `from` collection label, or a label produced by an earlier traversal).
- A `links` path that starts from an unbound label is **rejected**, even if it mentions a bound label later.

Rationale:

- This matches the current execution model in the compiler/VM: traversal needs a concrete base array so it can read parent ids and attach fields.
- It prevents a major source of confusion: reversing a pattern must not implicitly change what the compiler treats as the anchor.

Note: this anchoring rule is for `links` (select-time traversal). It does *not* restrict the `link` mutation, where node patterns are scans and can start from declarations.

#### Reference vs declaration inside paths

Within a node pattern:

- A **declaration/definition** (e.g. `users@u:User`, `books@b:Book`) introduces the collection label (`users`, `books`) and its element type.
- A **reference** (e.g. `(users)`, `(foods where ...)` when `foods` already exists in the same path) reuses the existing collection label. Untyped references do not carry `@binder` in the pipeline/header syntax; predicates use the binder introduced by a typed definer.

This matches the mental model: the label is “the whole array/many”, the `@binder` is “current element/one”.

#### Filters (`where ...`) are local to that segment

If a subsequent path adds `where ...` on a referenced node, the filter applies **only to that segment’s matching**. It must not retroactively change arrays established by earlier paths.

Consequence: nested fields still materialize; they are just empty when the filter excludes matches.

Merge rule (important when multiple `links` paths contribute to the same nested label):

- If a path contributes an **empty** array for a nested label on a parent element, it must **not** overwrite an existing **non-empty** array contributed by an earlier path.
- If multiple paths contribute **non-empty** arrays for the same nested label on the same parent element, the result is the **merge** of those arrays (currently treated as a unique/union-like merge in the execution model).

More precisely:

- A filter on the **base node** (the first node) gates whether the traversal runs for that parent element.
- A filter on a **later node** gates which reached nodes are attached.
- A filter never retroactively removes elements from arrays produced by earlier traversals.

#### Canonical examples (with expected auto-materialized shape)

These are “realistic” patterns intended to be mentally runnable.

##### Example 1: single-root traversal (reached labels are subsets)

Query:

```yed
select {
    users: users@u[*]
}
    from users@u:User
    links (users)->[writes:UserWritesBook]->(books@b:Book)
```

Expected auto-materialized shape:

```json
{
    "users": [
        {
            "writes": [
                { "books": [ { "id": "book:10" } ] }
            ]
        }
    ]
}
```

Note:

```text
Under strict locality, `books` exists under `writes` (the edge objects), not directly under the user.
This prevents confusion with any real `User.books` schema field.
```

If you also want a top-level “reached books” collection, compute it explicitly in the projection (example shape only):

```yed
select {
    users: users@u[*],
    reached_books: users@u[*].writes@w[**].books@b[**],
}
  from users@u:User
  links (users)->[writes:UserWritesBook]->(books@b:Book)
```

Expected shape:

```json
{
  "users": [ { "writes": [ { "books": [ { "id": "book:10" } ] } ] } ],
  "reached_books": [ { "id": "book:10" } ]
}
```

##### Example 2: continuation from an intermediate segment (single path)

Query:

```yed
select users@u[*]
    from users@u:User
        links (users)
            -> [writes:UserWritesBook]
            -> (books@b:Book where b.genre == "sci-fi")
            -> [cites:BookCitesBook]
            -> (cited_books@cb:Book)
```

Expected auto-materialized shape:

```json
{
    "users": [
        {
            "writes": [
                {
                    "books": [
                        {
                            "cites": [ { "cited_books": [ { "id": "book:20" } ] } ],
                        }
                    ]
                }
            ]
        }
    ]
}
```

Notes:

```text
- The filter `where b.genre == "sci-fi"` gates which books get a non-empty `cites` attachment.
- It does not retroactively remove upstream arrays; it only affects what this downstream segment contributes.
- The binder `b` is segment-local here. It does not become visible to outer tail clauses; outer filtering must stay root-local or use an explicit root-anchored reduction / nested query.
```

##### Example 3: base-node filter gates the traversal

Query:

```yed
select users@u[*]
    from users@u:User
    links (users where u.active == true)->[writes:UserWritesBook]->(books@b:Book)
```

Expected auto-materialized shape:

```json
{
    "users": [
        { "active": true,  "writes": [ { "books": [ { "id": "book:10" } ] } ] },
        { "active": false, "writes": [] }
    ]
}
```

##### Example 4: true multi-root (two independent scans)

Implementation note:
- This example describes intended semantics.
- Some Flow lowering entrypoints are currently single-root.
- However, multi-root planning may still apply Flow/QIR optimizations per root when the relevant parts of the query can be represented as a single-root Flow plan.

Query:

```yed
select {
    users: users@u[*],
    books: books@b[*],
}
    from users@u:User, books@b:Book
    links (users)->[writes:UserWritesBook]->(books)
    links (books)<-[written_by:UserWritesBook]<-(users)
```

Expected auto-materialized shape:

```json
{
    "users": [
        { "writes": [ { "books": [ { "id": "book:10" } ] } ] }
    ],
    "books": [
        { "written_by": [ { "users": [ { "id": "user:1" } ] } ] }
    ]
}
```

Important: in multi-root form, `books` is the full `from books@b:Book` scan (subject to its own filters/range), not only the reached subset.

To emphasize the difference under strict locality:

```text
users[*].writes[*].books   = books reached from each user (per-edge view)
books[*]                  = the independent books root scan (global view)
```

##### Scenario A: subsequent path is a reference + extra filter

```yed
from users@u:User
links (users)->[likes:UserLikesFood]->(foods@f:Food),
    (foods where f.age > 5)<-[eaten_by:UserEatsFood]<-(other_users@ou:User)
```

Materialization (schematic):

```text
users: [
    {
        likes: [
            {
                foods: [
                    {
                        eaten_by: [
                            {
                                users: [ ... ]
                            }
                        ]
                    }
                ]
            }
        ]
    }
]
```

Why:

- The first path introduces `users -> likes -> foods`.
- The second path anchors at `foods` (already introduced by the earlier sibling path) and attaches `eaten_by -> users` under each food.
- The `where f.age > 5` filters which foods get a non-empty `eaten_by`; it does not filter away foods produced by the first path.

##### Scenario B: subsequent path starts with a fresh declaration

```yed
from users@u:User
(users)->[likes:UserLikesFood]->(foods@f:Food),
(books@b:Book)<-[written_by@wrb:UserWritesBook]<-(users@u2)
```

This is **rejected** in `select ... links ...` under the anchoring rule, because it starts from `books` which is not yet a bound base label in this query.

Write the anchored form explicitly instead:

```yed
from users@u:User
(users)->[likes:UserLikesFood]->(foods@f:Food),
(users@u2)->[written_by@wrb:UserWritesBook]->(books@b:Book)
```

Materialization (schematic):

```text
users: [
    {
        likes: [ { foods: [ ... ] } ],
        written_by: [ { books: [ ... ] } ],
    }
]
```

Why:

- The base is `users`, so `written_by` attaches under each user.
- Under strict locality, `books` attaches under `written_by` (the edge objects), not directly under the user.
- `@u2` is an iteration binder for filters; it does not rename output keys.

Note on naming:

- The output keys come from labels (`users`, `books`, `written_by`), not from binder names (`@u2`).
- If you want the endpoint label to be `other_users`, name it explicitly: `(other_users:User)`.

##### Scenario C: declaration branch + later reference has an extra filter

```yed
from users@u:User
(users)->[likes:UserLikesFood]->(foods@f:Food),
(books@b:Book)<-[written_by@wrb:UserWritesBook]<-(users@u2 where u2.age > 5)
```

This is also **rejected** for the same reason as Scenario B.

Use the anchored form:

```yed
from users@u:User
(users)->[likes:UserLikesFood]->(foods@f:Food),
(users@u2 where u2.age > 5)->[written_by@wrb:UserWritesBook]->(books@b:Book)
```

Materialization is the same as the anchored Scenario B, except users that fail the base-node predicate get `written_by: []` from this branch (and therefore no nested `books`).

##### Scenario D: “A vs B” when the endpoint is a pure reference

```yed
from users@u:User
(users)->[likes:UserLikesFood]->(foods@f:Food),
(foods)<-[eaten_by:UserEatsFood]<-(users)
```

Materialization corresponds to your **A**:

```text
users: [
    {
        likes: [
            {
                foods: [
                    {
                        eaten_by: [
                            {
                                users: [ ... ]
                            }
                        ]
                    }
                ]
            }
        ]
    }
]
```

Why:

- A reference does not “turn off” materialization; it only means “reuse the existing binding/type”.
- The hop to `(users)` is still a traversal hop, so it materializes a nested endpoint array keyed by the label (`users`).

If you want a different nested key name, rename the label explicitly:

```yed
(foods)<-[eaten_by:UserEatsFood]<-(other_users:User)
```

Then materialization uses `other_users: [ ... ]` at that point.

#### Rejected forms (by design)

These are rejected in `select ... links ...` to keep anchoring/materialization unambiguous:

1) Unbound base label:

```yed
from users@u:User
links (books@b:Book)->[written_by:UserWritesBook]->(users)
```

Reason: `books` is not a bound base array in this query.

2) “Looks anchored later” (still rejected):

```yed
from users@u:User
links (books@b:Book)<-[written_by:UserWritesBook]<-(users)
```

Reason: we do not implicitly rotate/re-anchor based on later mentions; the first node is the anchor.

Why reject at all?

Because `links` is defined as a *correlated traversal* that materializes onto an upstream array (it is not a general join between two independent scans). Without a single explicit base array, the compiler has to guess between incompatible interpretations:

- **Implicit re-anchoring/rotation**: treat `(books ...)<-...<-(users)` as if it were written starting from `users`.
- **Multi-root scan**: treat `(books@b:Book)` as a second independent root and run a traversal over it.
- **Join/cross-product**: scan both tables and correlate them (potentially huge) before materializing.

Those have different runtime costs, different result shapes, and different “what does `select` return?” implications. So the spec requires that `links` paths start from an already-bound label.

What would your example materialize to if we *did* allow implicit re-anchoring?

For:

```yed
from users@u:User
links (books@b:Book)<-[written_by:UserWritesBook]<-(users)
```

the only sensible anchored interpretation (since `users` is the bound base) would be equivalent to:

```yed
links (users)->[written_by:UserWritesBook]->(books@b:Book)
```

and therefore materialize under `users` (not produce a separate top-level `books`):

```text
{
    users: [
        {
            written_by: [ { books: [ ... ] } ],
        }
    ]
}
```

What about multi-source?

If we want true multi-source queries, it should be *explicit* and should make the output shape obvious. One clear design is:

- Allow multiple `from` roots: `from users@u:User, books@b:Book`
- Require `select` to return an object with named root fields (so there is no ambiguity about returning `[T]` vs `{...}`):

```yed
select {
    users: users@u[*],
    books: books@b[*],
    }
    from users@u:User, books@b:Book
    links (users)->[write:UserWritesBook]->(books)
    links (books)<-[written_by:UserWritesBook]<-(users)
```

In this model, both roots are real, and `links` can start from either root because both are explicitly bound.

How to get what you want:

- To materialize under `users`, write the anchored form:

```yed
links (users)->[written_by:UserWritesBook]->(books@b:Book)
```

- To materialize under `books`, bind books as the `from` root (i.e. a separate query):

```yed
select books@b[*]
    from books@b:Book
    links (books)<-[written_by:UserWritesBook]<-(users@u:User)
```

## 6) Aggregations over arrays

If you want “answer questions about the array” without a special DSL, keep aggregations as normal functions.

Examples:

```py
len(users)
count(users)
sum(users@u[*].age)
avg(users@u[*].age)
```

If you also want a namespaced style, `math::count(users)` is fine, but `len(users)` should probably remain the canonical primitive.

## 7) Keyword choice: `return` vs `yield`

For non-`select` queries (e.g. `create`, `update`) a mutation-level clause named `return` reads like function return.

To avoid that confusion, mutation results are now specified with a keyword-leading block whose value is the **tail expression**:

```yed
create { users@u:User { id: 1 }; users@x[*].{ id: x.id } };
update { users@u:User set u.name = 2 where u.id == 1; users@x[*].{ id: x.id } };
```

In other words: mutations don’t “return” via a special clause; they optionally compute a value via `; <expr>` inside the mutation block.

If we later add a general `query { ... }` block (multiple mutations + tail expression), it can follow the same “tail expression is the value” rule.

## 8) Mutations (locked-in syntax + semantics)

This section is the “decision” version: it’s the target surface syntax and scoping model for implementing mutations end-to-end.

### 8.1 Two forms: statement vs block

Every mutation kind supports:

- **Statement form**: performs the mutation and yields a default result.
- **Block form**: performs the mutation and yields the **tail expression** after `;`.

Examples:

```yed
// statement form: default result
update users@u:User set u.age = u.age + 1 where u.age >= 18

// block form: tail expression is the value
update { users@u:User set u.age = u.age + 1 where u.age >= 18; 123 }
```

### 8.2 Binders and scope

Mutations can introduce two kinds of names:

- **Collection label**: the “main” label (e.g. `users` in `users@u:User`).
    - Visible in the tail expression of the mutation block.
    - Often typed as a collection (`[T]`) when operating on many.
- **Item binder**: introduced with `@` (e.g. `u` in `users@u:User`).
    - Visible only inside the mutation statement itself (including nested `where` filters inside traversals).
    - **Not** visible in the mutation tail expression.

This is the key boundary rule: the tail expression can use the “main collection labels”, but **cannot** use any `@item` binders.

### 8.3 Root-first boundary rule for collections

If a name is a collection value (`[T]`), then you must explicitly enter an element boundary before:

- member access: `x.field`
- document projection: `x.{ ... }`

Use `[*]` (or another explicit mapping/binding form) to enter the boundary:

```yed
users[*].age
users@u[*].{ id: u.id }
```

This is what enforces the “`users[*].follows` is different from `follows`” mental model.

### 8.4 Default results (when not using a block tail)

- `create` / `upsert`: default to the inserted/upserted record(s)
    - single payload => `T`
    - array payload => `[T]`
- `update` / `delete` / `link` / `unlink`: default to the affected row count (`i64`)

### 8.5 CREATE

```yed
// one
create user@u:User { id: "User:1", age: 1 }

// many
create users@u:User [
    { id: "User:1", age: 1 },
    { id: "User:2", age: 2 },
]

// block tail overrides the default result
create { users@u:User [{ id: "User:1", age: 1 }]; users }
```

Notes:

- `users` is the collection label; `u` is statement-scoped only.

### 8.6 UPSERT

Same surface shape as `create`.

```yed
upsert user@u:User { id: "User:1", age: 10 }

upsert users@u:User [
    { id: "User:1", age: 10 },
    { id: "User:2", age: 20 },
]

upsert { users@u:User [{ id: "User:1", age: 10 }]; users[*].age }
```

### 8.7 UPDATE

Update is match-set driven (the `where` clause can select 0..N rows). There is no “array payload” for update; instead the RHS expressions compute new values per matched row.

Two equivalent ways to spell updates:

**(A) Setter form**

```yed
// one setter
update users@u:User set u.age = u.age + 1 where u.age >= 18

// multiple setters
update users@u:User set {
    u.age = u.age + 1;
    u.id = u.id;
} where u.age >= 18
```

**(B) Patch object form** (sugar for multiple setters)

```yed
update users@u:User {
    age: u.age + 1,
} where u.age >= 18
```

Design verdict:

- We do **not** need separate `merge`/`replace` keywords for update.
    - The patch object form expresses “update these fields”.
    - If/when a full replace is desired, it can be expressed as setting every field (or handled via UPSERT conflict strategy).

Batch update with explicit collection + item binder:

```yed
update users@u:User set u.age = u.age + 1 where u.age >= 18

// In a block tail: `u` is out of scope, but `users` is in scope.
update { users@u:User set age = u.age + 1 where u.age >= 18; users[*].age }
```

### 8.8 DELETE

```yed
delete users@u:User where true

delete { users@u:User where u.age < 18; 123 }
```

Default result is `i64` (count). A block tail can compute any value.

### 8.9 LINK / UNLINK

LINK creates edge rows; UNLINK deletes edge rows.

```yed
link (users@u:User) -> [follows@f:UserFollowsUser {}] -> (targets@t:User)

unlink (users@u:User) <-> [follows@f:UserFollowsUser hops 1..3] <-> (targets@t:User)
```

Binder rules:

- Traversal labels like `users`, `targets`, `follows` are treated as collection labels (`[T]`).
- Any `@item` binders (`u`, `t`, etc.) are statement-scoped only and do not leak into the tail.

So, in a tail expression:

```yed
// invalid: collection projection needs an explicit element boundary
link { (users@u:User) -> [follows@f:UserFollowsUser {}] -> (targets@t:User); users[*].{ id } }

// valid
link { (users@u:User) -> [follows@f:UserFollowsUser {}] -> (targets@t:User); users@x[*].{ id: x.id } }
```
