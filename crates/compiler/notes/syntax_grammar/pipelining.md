# Pipelining (FROM / LINKS / WHERE / GROUP / ORDER / RANGE)

Status: proposal-level semantics.

This document is dedicated to the **pipeline** of a `select` query: how collections are introduced (`from`), how correlated traversals attach nested results (`links`), and how global pipeline transforms (`where`, `group by`, `order by`, `range`) compose.

It intentionally focuses on *query-shape and scoping* rather than execution strategies.

Related (canonical) docs:
- `src/syntax/semantics.md` — path expressions, selectors (`[*]`, `[where]`, `[**]`), and the “nested materialization” model.
- `src/syntax/name-resolution-and-scoping.md` — identity rules, what counts as an introduction vs reference, and `links` anchoring rules.
- `src/syntax/select.md` — illustrative end-to-end examples.
- `src/syntax/nested-array-navigation.md` — nested array shape intuition and common patterns.

---

## 0) Mental model (one sentence)

- The **projection** (`select <expr>`) determines the output value/shape.
- The **pipeline** (`from/links/where/group/order/range`) determines what *names* exist, and what *collections* those names denote.

Surface contract:

- `from` is required for `select`.
- The language does not have a rootless `select 1` form.
- If there is no `from`, there is no query source and no pipeline-introduced names to evaluate against.
- `select 1 from users@u:User` still returns `1`; `from` introduces context, not implicit row-shaped output.
- `users@u[*].best_friend?.id` keeps `?` as projection-level optional chaining rather than turning it into a special pipeline/null-row rule.

Corollary:
- There is **no implicit row mapping** from `from` into the projection. If you want to compute per-element results, you must do so explicitly with selectors like `[*]` / `[where ...]` / `[n]` / slices / `[**]`.
- For query/path collections, selector index and slice remain order-sensitive: explicit path order makes them deterministic, while bare unordered selectors are valid with arbitrary engine-chosen order.

Implementation note (streaming unit hint):
- If the final projection uses selector flattening (`[**]` or deeper), physical lowering may choose a leaf-emission unit (`PhysStreamUnit::Leaf`) even though the semantic output is still the same nested value.

---

## 1) Names in the pipeline (quick recap)

Within a statement there are two name classes:

- **Collection labels** (pre-`@`): `users`, `writes`, `books`, `groups`.
  - Denote **many** values (typically `Array<T>`).
  - Used for whole-collection operations: `len(users)`, slicing, etc.

- **Item binders** (post-`@`): `@u`, `@w`, `@b`.
  - Denote a **one** value (the “current element”) inside selector/predicate contexts.

Identity constraints (statement-wide):
- A collection label is introduced once.
- A pipeline binder identity is introduced once.

For the full rules, see `src/syntax/name-resolution-and-scoping.md`.

---

## 2) Clause order (logical)

A typical `select` statement is logically:

```yed
select <projection-expr>
from <from-items>
[links <links-paths>]
[where <pipeline-predicate>]
[group by { <key>: <expr>, ... } into <label>]
[order by ...]
[range <range-expr>]
```

Notes:
- This is a **logical** description, not a mandate on runtime execution.
- Even if the runtime streams per-parent, the observed result must match this meaning.

Current lowering note for `range`:

```text
surface syntax accepts full expressions for top-level `range`
constant bounds stay in the query-range carrier for planner-visible lowering
non-literal SELECT-level bounds currently lower as an explicit runtime slice over the query result value
root-scan and post-`links` pipeline ranges are still planner-native literal bounds today
```

So:

```yed
range ..u.writes@w[*].books@b[**].count()
```

is a valid surface idea, but today it is not yet a planner-native query-range operator.

More explicitly:

```text
top-level SELECT result range
  -> may be non-literal
  -> non-literal form currently desugars to value-level slicing of the final query result

pipeline/root/post-links range
  -> still lowers through source-shaped `QueryRange { start, end, inclusive_end }`
  -> currently requires non-negative integer literals
```

---

## 3) FROM: introducing root collections

### 3.1 Single root scan

```yed
select users@u[*].{ id, name }
from users@u:User
```

- Introduces label `users` (a collection).
- Introduces binder identity `u` for element contexts.

Note: the binder identity does not imply an “ambient current row” in the projection.
The projection is an expression evaluated over the names introduced by the pipeline.

Auto-materialized shape (schematic):

```text
users: [ { ...User fields... }, ... ]
```

Contrast with a scalar projection:

```yed
// Projection is scalar, so the whole query result is scalar.
select 3
from users@u:User
```

### 3.2 Root filtering (recommended: in `from`)

```yed
select users@u[*].{ id, name }
from users@u:User where u.active == true
```

This filters which items appear in the `users` root collection.

### 3.3 Multiple roots

```yed
select {
  users: users@u[*].id,
  books: books@b[*].id,
}
from users@u:User, books@b:Book
```

Recommended semantics:
- Multiple `from` items introduce multiple independent root collections.
- There is **no implicit join/cross-product** at the surface language; correlation is expressed via `links` or explicit expressions.

Implementation note:
- The intended semantics above are spec-level.
- Today, some Flow lowering entrypoints still assume a single-root SELECT.
- However, multi-root planning can still opportunistically run Flow/QIR optimizations per root (i.e. optimize each root independently) when the relevant predicates can be expressed in a single-root Flow plan.

(If you ever want a cross product, it should be explicit as an operator/function so it’s never accidental.)

Auto-materialized shape (schematic):

```text
users: [ ... ],
books: [ ... ]
```

### 3.4 Facet-style results (projection subqueries)

If you want **multiple independent result collections** ("facets"), prefer returning a struct/object whose fields are populated by **independent nested `select` expressions**.

Why this is recommended:
- Each nested `select` is a normal single-root query, so global `where/group/order/range` are unambiguous.
- You avoid multi-root tail-stage ambiguity without having to use multi-root disambiguation syntax.
- The optimizer is free to lift/decorrelate/cse internally, but the surface language stays explicit.

Example:

```yed
@table
struct User { id: i64 }

@table
struct Post { id: i64, user_id: i64 }

struct Facet { users: [i64], post_ids: [i64] }

fn main() -> Facet {
  Facet {
    users: (select users@u[order by u.id].id from users@u:User),
    post_ids: (select posts@p[order by p.id].id from posts@p:Post),
  }
}
```

Facet-local correlation works as usual inside each query:

```yed
struct Facet { users: [{ id: i64, has_posts: bool }], post_ids: [i64] }

fn main() -> Facet {
  Facet {
    users: (
      select users@u[order by u.id].{
        id: u.id,
        has_posts: !(select posts@p[*].id
                     from posts@p:Post
                     where p.user_id == u.id).is_empty(),
      }
        from users@u:User
    ),
    post_ids: (select posts@p[order by p.id].id from posts@p:Post),
  }
}
```

Guideline:
- If two facets genuinely need to share a traversal, express that shared work in **one** `select` (typically via `links`) and project multiple arrays out of the same materialized structure.
- If sharing would require a new binding form (e.g. `let x = <select ...>` used in multiple facets), prefer duplication first and let the optimizer improve it later.

---

## 4) LINKS: correlated traversal that materializes nested arrays

### 4.1 What `links` is (and is not)

- `links` is **correlated traversal** when it includes an explicit edge hop (`-> [edge] ->`).
- It **materializes nested arrays** (virtual nested fields) at the syntactic level where labels are written.
- It is **not** a join/tuple model at the surface syntax.

Implementation note:
- The compiler/runtime may execute a `links` traversal either via a dedicated traversal operator or by compiling it into an explicit join-chain substrate (joins + per-parent nest materialization) late in lowering.
- This must preserve the same observable behavior (strict locality, multiplicity, and ordering when it is observable via indexing/scalar extraction).

In addition, a path may include a **node-to-node hop** (no edge segment). This is a deliberate convenience for “attach this collection here” patterns.

Node-to-node hop semantics (proposal):
- `(a)->(b)` materializes the collection `b` under each parent element of `a`.
- This hop is *not* correlation by id; it is a broadcast/attachment.
- If you want a flat cross-product of pairs, express it in the projection (e.g. nested maps + `[**]`).

### 4.2 The anchor rule (base node must be a reference)

In `select ... links ...`, the first node in each path is the **anchor**.

Allowed:

```yed
from users@u:User
links (users)->[writes@w:UserWritesBook]->(books@b:Book)
```

Not allowed (typed anchor introduces/redeclares a source):

```yed
from users@u:User
links (users@u:User)->[writes@w:UserWritesBook]->(books@b:Book)
```

### 4.3 Segment predicates are filters (not new identities)

```yed
from users@u:User
links (users where u.age > 50)->[writes@w:UserWritesBook]->(books@b:Book)
```

- `where` on `(users ...)` gates whether traversal runs for a given user.
- It does not remove users from `users` (it only affects what this `links` contributes).

### 4.4 Direction operators

Assuming edge endpoints `_from` and `_to`:
- `->` matches `_from == parent.id` and reaches `_to`.
- `<-` matches `_to == parent.id` and reaches `_from`.
- `<->` matches either direction; the “other endpoint” is whichever is not `parent.id`.

### 4.5 Strict locality (materialization level)

For:

```yed
links (users)->[writes:UserWritesBook]->(books@b:Book)
```

- `writes` attaches under each user.
- `books` attaches under each edge inside `writes`.
- `books` is **not** automatically hoisted onto the user.

If you want “all reached books per user”, compute it in projection:

```yed
select users@u[*].{
  id,
  reached_books: u.writes@w[*].books@b[**],
}
from users@u:User
links (users)->[writes@w:UserWritesBook]->(books@b:Book)
```

Auto-materialized shape for the `links` itself (schematic):

```text
users: [
  {
    writes: [
      {
        books: [ { ...Book fields... }, ... ]
      },
      ...
    ]
  },
  ...
]
```

In practice:
- Redeclaring/redefining an item binder (e.g. `from users@u:User, friends@u:User`) is a compile error.
- Redeclaring/redefining a collection label is treated as an error by the spec because it makes whole-collection references ambiguous (e.g. which `users` does `len(users)` mean?).

### 4.6 Union in an edge hop (`|`)

If you want “either of these edge types/collections”, prefer a **single edge segment** whose type is a union:

```yed
from users@u:User
links (users)->[actions@e:(UserWriteBook | UserReadBook)]->(books@b:Book)
```

This keeps naming predictable:
- `actions` is the single nested edge array under each user.
- `e` is the single binder used in predicates/projection for that edge.

Avoid putting distinct labels/binders inside one union segment like:

```text
(users)->[write@w:UserWriteBook | read@r:UserReadBook]->(books@b:Book)
```

Projection note:
- With `actions@e:(UserWriteBook | UserReadBook)`, the element binder `e` has a **union type**.
- If you only need fields common to both edge types (e.g. `_from`, `_to`, shared metadata), you can project directly.
- If you want to *split* actions into separate arrays (writes vs reads) without duplicating the traversal, you need a type guard / match construct.

One possible idiom (requires a type-test primitive such as `is(e, Type)`):

```yed
select users@u[*].{
  id: u.id,
  written_books: u.actions@e[where is(e, UserWriteBook)].books@b[**],
  read_books: u.actions@e[where is(e, UserReadBook)].books@b[**],
}
from users@u:User
links (users)->[actions@e:(UserWriteBook | UserReadBook)]->(books@b:Book)
```

If you do not want to add a type-test primitive yet, the non-tricky alternative is to write two explicit `links` paths with distinct labels and accept that the engine may optimize shared work.

because it creates ambiguous scope (which binder is in scope?) and conflicts with single-assignment naming.
If you truly need distinct labels/binders, write two comma-separated `links` paths (same prefix, different segment labels), and combine results explicitly in projection.

---

## 5) Multiple LINKS paths and branching scenarios

There are two distinct "multi" concepts:

### 5.1 Multiple paths inside one `links` clause (comma-separated)

```yed
from users@u:User
links
  (users)->[writes@w:UserWritesBook]->(books@b:Book),
  (users)->[befriends@bf:UserBefriendsUser]->(friends@f:User)
```

Rule (intended):
- Paths are interpreted **left-to-right**.
- Labels introduced by earlier sibling paths are in scope for later sibling paths.
- Therefore, you *can* start a later sibling path from an intermediate label introduced earlier in the same `links` clause.

Example: reuse `foods` reached by the first path:

```yed
from users@u:User
links
  (users)->[likes:UserLikesFood]->(foods@f:Food),
  (foods where f.calories > 500)<-[eaten_by:UserEatsFood]<-(other_users@ou:User)
```

Auto-materialized shape (schematic):

```text
users: [
  {
    likes: [
      {
        foods: [
          {
            eaten_by: [ { other_users: [ ... ] }, ... ]
          },
          ...
        ]
          What this means in practice:
          - `(users)` is a **reference** to an in-scope collection label introduced earlier (usually by `from`).
          - Because it is a reference, it **cannot** change identity: you cannot add a new binder, and you cannot add a type constraint.
          - You *can* add a segment predicate/modifier to filter what flows through this traversal, e.g. `(users where u.active)`.
            - This uses the **existing** item binder (`u`) from `from users@u:User`.
            - It does **not** filter the root `users` collection; it only affects what the `links` path materializes for each parent.
            - If this (or any later sibling path) materializes an empty nested array for a given parent, it must not overwrite an existing non-empty nested array produced by an earlier sibling path; when both contribute values, arrays are merged.
            - Current implementation note: the start node supports `where` only (ORDER/RANGE on the start node are currently rejected).
      }
    ]
  }
]
```

### 5.2 Branching from an intermediate segment (sharing the intermediate)

If the intent is to branch from an intermediate result (e.g. reuse `liked_foods`), you can anchor later sibling paths at that intermediate label instead of restarting from the root:

```yed
from users@u:User
links
  (users)->[likes@l:UserLikesFood]->(liked_foods@lf:Food),
  (liked_foods)->[has_ingredient@hi:FoodHasIngredient]->(ingredients@i:Ingredient),
  (liked_foods)->[served_at@sa:FoodServedAt]->(restaurants@r:Restaurant)
```

This reuses the same `liked_foods` anchor rather than repeating the `users -> likes -> liked_foods` prefix.

---

## 6) WHERE: pipeline filter vs selector filter

There are two common "where" shapes:

### 6.1 Selector filter (expression-level)

```yed
select users@u[where u.age > 18][*].{ id, age }
from users@u:User
```

- Filters the `users` array value in the expression.

### 6.2 Pipeline `where` (clause-level)

```yed
select users@u[*].{ id, age }
from users@u:User
where u.age > 18
```

- Filters the current pipeline root collection(s) after `from`/`links`.
- Use this when you want the rest of the pipeline (group/order/range) to see the filtered stream.

Common idiom:
- Existential/universal predicates over nested collections, e.g. `(<subquery>).any(|row| pred(row))` and `(<subquery>).all(|row| pred(row))`.
  - The closure parameter is the current element (the collection’s element type); outer pipeline bindings are still in scope as captures.
    - If the collection elements are objects, the closure param is that object.
    - If the collection elements are scalars (because the projection selects a scalar), the closure param is that scalar.
  - Predicate short-circuiting is semantically observable when the predicate can error (e.g. indexing), so implementations must preserve the meaning even if they optimize/decorrelate.

Implementation status note:
- `is_empty()` is executable in the VM.
- `any/all/none` are executable in the VM for arrays; in query contexts, planning may also
  decorrelate some forms into join operators.

Performance/clarity note:
- For `links`-heavy queries, it is often cleaner (and easier to optimize) to push predicates into
  `links` segment/node predicates or use selector filtering + `is_empty()`.

Recommended rule of thumb:
- Use `from ... where ...` for base scans.
- Use `links (label where ...)` / `[segment where ...]` / `(node where ...)` for traversal-local filters.
- Use pipeline `where` for global filtering after the pipeline has been built.

### 6.3 Stage-affecting vs value-level modifiers (the core rule)

This is the key surface-semantics choice:

- **Stage-affecting modifiers** change what later pipeline stages see.
  - Examples: `from (users@u:User where ... order by ... range ...)`, segment predicates in `links`, and `group by` itself.
  - These modifiers are the *semantic* way to express “do X before `links`” or “do X before `group by`”.

- **Value-level selectors** (inside expressions, including the projection) transform an already-existing collection value.
  - Examples: `users@u[where ...][order by ...][0..=10]...`, `groups@g[where ...][order by ...]...`.
  - These do **not** retroactively constrain earlier pipeline stages. They shape the value you project.

This rule keeps evaluation order predictable:

```text
FROM/links/group build collections  ->  projection selects/shapes values
```

Optimization note (non-normative): the compiler is allowed to *recognize* certain value-level selector patterns and lower them into equivalent stage-affecting pipeline operators when it is semantics-preserving (e.g. pushing a selector-range into `from`), but users should not rely on that for correctness.

#### Example: A and B can return the same value, but B is the semantic “pre-links” form

These two queries may produce the same final output, but they are different *pipeline meanings*:

**A (value-level post-stage shaping):**

```yed
select users@u[order by u.name][0..=10].{
  id,
  books: u.writes@w[*].books@b[**]
}
from users@u:User
links (users)->[writes@w:UserWritesBook]->(books@b:Book)
```

**B (stage-affecting pre-links shaping):**

```yed
select users@u[*].{
  id,
  books: u.writes@w[*].books@b[**]
}
from (users@u:User order by u.name range 0..=10)
links (users)->[writes@w:UserWritesBook]->(books@b:Book)
```

Guideline:
- If you need to guarantee that `links` (and any later pipeline stage like `group by`) runs only for the top-k users, write B.
- If you only need to shape what is returned (and you do not care whether the engine computes link materialization for additional users internally), A is fine.

#### “Downstream-dependent” filtering

Sometimes you want to filter a root based on linked/nested data (e.g. “users who wrote a Tech book”). There are two stable ways to express this:

1) **Post-links value-level filter** (when it’s fine to build links first):

```yed
select users@u[where len(u.writes@w[*].books@b[**][where b.genre == "Tech"]) > 0][*].{ id }
from users@u:User
links (users)->[writes@w:UserWritesBook]->(books@b:Book)
```

2) **Stage-affecting filter via correlated subquery** (when you need the filtered stream to feed later stages like grouping/pagination):

```yed
select users@u[*].{ id }
from (users@u:User where !(
    select users[*]
    from books@b:Book
    links (users)->[writes@w:UserWritesBook]->(books)
    where b.genre == "Tech"
  ).is_empty()
)
```

The correlated-subquery form is explicit about phase ordering and is the easiest for the optimizer/decorrelator to reason about.

Post-`links` tail stages stay root-local. Link-segment binders such as `@w` / `@b` are available inside the segment that introduced them, but they do not leak into outer `where` / `group by` / `order by`. When you need to filter the parent stream based on linked children, write that as a root-anchored reduction like `u.writes@w[*].books@b[**].any(|b| b.genre == "Tech")` or as an explicit nested query.

---

## 7) GROUP BY: producing groups (and “having”)

Key point:
- `group by` is a **pipeline transform** that *constructs* a new collection of **group objects** (keys + members).
- Once you have a grouped collection, it behaves like any other collection value: you can filter it, order it, slice it, and map it using expression-level selectors.
  - This is why “HAVING” is modeled as filtering the `groups` collection in projection.

Non-goal:
- This does **not** imply we should eliminate global pipeline stages (`where/order/range`). Those stages are still the most direct way to define the *main stream* of a single-root query.
  - Selector-level `[where]` / `[order by]` / slicing are great for **nested arrays** and **post-materialization shaping**.
  - Pipeline stages are for shaping the **current pipeline stream** so that subsequent pipeline stages see the same filtered/ordered/paginated stream.

### 7.0 Grouping target in multi-root `from` (keyword-free)

When `from` introduces multiple roots (e.g. `users@u:User, books@b:Book`), each grouping specification must decide **which root stream** it groups.

Proposed rule (no extra keywords):
- Determine which *pipeline root binders* are referenced by the group key expressions.
- If the keys reference exactly one root binder (e.g. only `u`), then `group by` groups that root (`users`).
- If the keys reference no root binders (all keys are constants), default to the first `from` item.
- If the keys reference multiple root binders (e.g. both `u` and `b`), it is a compile error; write an explicit cross/flatten in the projection (or use a dedicated operator) before grouping.

This keeps `group by { city: u.city } into groups` visually simple while remaining predictable.

### 7.1 One `group by`, multiple group specs (comma-separated)

To compute multiple independent grouped collections in the same query, use a single `group by` clause with comma-separated group specs:

```yed
group by { city: u.city } into city_groups,
         { country: u.country } into country_groups
```

Each group spec:
- chooses its grouping target using the binder-inference rule in §7.0,
- produces a collection named by `into <name>`.

### 7.2 Grouping a root (single key)

```yed
select groups@g[*].{
  city: g.city,
  users: g.users@u[*].id,
}
from users@u:User
group by { city: u.city } into groups
```

Semantics:
- `group by` transforms the root stream into a `groups` collection.
- Each group exposes the key fields and the grouped members via the original root label(s) (e.g. `g.users`).

Resulting object shape (schematic):

```text
groups: [
  { city: "Paris", users: [ ...User... ] },
  { city: "London", users: [ ...User... ] },
]
```

### 7.3 “Having” is just filtering groups

```yed
select groups@g[where len(g.users) > 10][*].{ city: g.city }
from users@u:User
group by { city: u.city } into groups
```

You can similarly order/slice groups in projection:

```yed
select groups@g[order by g.city asc][0..=10][*].{ city: g.city }
from users@u:User
group by { city: u.city } into groups
```

### 7.4 Grouping by multiple fields (composite key)

```yed
select groups@g[*].{
  city: g.city,
  country: g.country,
  users: g.users@u[*].id,
}
from users@u:User
group by { city: u.city, country: u.country } into groups
```

Resulting object shape (schematic):

```text
groups: [
  { city: "Paris", country: "FR", users: [ ... ] },
  { city: "Paris", country: "US", users: [ ... ] },
]
```

### 7.5 Grouping by computed keys (not just field access)

Example A: normalize + bucketize

```yed
select groups@g[*].{
  city_lower: g.city_lower,
  decade: g.decade,
  users: g.users@u[*].{ id, age },
}
from users@u:User
group by {
  city_lower: string::lower(u.city),
  decade: (u.age / 10) * 10,
} into groups
```

Resulting object shape (schematic):

```text
groups: [
  { city_lower: "london", decade: 20, users: [ ... ] },
  { city_lower: "london", decade: 30, users: [ ... ] },
]
```

Example B: derive key from linked/nested values (summary)

```yed
select groups@g[*].{
  wrote_any: g.wrote_any,
  users: g.users@u[*].id,
}
from users@u:User
links (users)->[writes@w:UserWritesBook]->(books@b:Book)
group by {
  wrote_any: len(u.writes@w[*].books@b[**]) > 0,
} into groups
```

### 7.6 Multiple group specs in a multi-root query

This is how you “use two groups together” in one query without repeating `group by`:

```yed
select {
  user_groups: user_groups@g[*].{ city: g.city, users: g.users@u[*].id },
  book_groups: book_groups@bg[*].{ genre: bg.genre, books: bg.books@b[*].id },
}
from users@u:User, books@b:Book
group by { city: u.city } into user_groups,
         { genre: b.genre } into book_groups
```

Resulting object shape (schematic):

```text
{
  user_groups: [ { city: "Paris", users: [ ... ] }, ... ],
  book_groups: [ { genre: "Tech", books: [ ... ] }, ... ],
}
```

### 7.7 Grouping by derived scalar from linked/nested values

Because `links` materializes nested arrays, you can group by summaries:

```yed
select groups@g[*].{
  genre: g.genre,
  users: g.users@u[*].id,
}
from users@u:User
links (users)->[writes@w:UserWritesBook]->(books@b:Book)
// group users by how many books they wrote in the traversal
group by { genre: first(u.writes@w[*].books@b[**].genre) } into groups
```

Guideline:
- Group keys should be **one scalar per root item**.
- If you need to group by “many” values, flatten/re-root explicitly (see gaps below).

Auto-materialized shape after grouping (schematic):

```text
groups: [
  {
    genre: "Tech",
    users: [ ...users in this group... ]
  },
  ...
]
```

`group by` always produces **group objects** (key + members) and therefore requires an explicit `into <label>` target.

Type restriction (current): group-by keys must be made of orderable primitives (currently i64/bool/string/unit).

Scoping rule:
- Group key expressions are evaluated **before grouping**, per input row, using the in-scope FROM binders (e.g. `u`).
- The group iterator binder (e.g. `groups@g[*]`) exists only **after** grouping, in the projection that consumes the `into` collection.
  - Therefore, `g` cannot appear inside `group by { ... }` keys.

If you want per-group derived fields (like `stats.total_low`, `g.total_pages`, etc), compute them in a post-group projection over the group objects:

```yed
let enriched = select groups@g[*].{
  city: g.city,
  total: g.users@u[*].age.sum(),
}
from users@u:User
group by { city: u.city } into groups;

enriched@e[*].{ city: e.city, total: e.total }
```
```

---

## 8) ORDER / RANGE: global vs nested

### 8.1 Global ordering + pagination

```yed
select users@u[*].{ id, age }
from users@u:User
order by u.age desc
range 20..30
```

Notes:
- Global `order/range` are defined on the **pipeline stream**.
- Ordering is only semantically guaranteed when an explicit `order by` clause is present.
  - If `order by` is absent, the engine may return results in any order; programs should not rely on incidental stability.
  - If `order by` is present and keys compare equal (ties), the implementation preserves original row order (stable sort) to keep results deterministic.
- Implementation note: in a distributed executor, a global `order by` typically implies a global coordination barrier (e.g. a gather/merge exchange) even if upstream operators are parallel.
- Implementation note: the physical plan can contain explicit `Exchange` ops (`RepartitionBy` / `Broadcast` / `Gather` / `Merge`).
  - In single-node execution, these act as semantics-preserving planning boundaries.
  - In the sharded/mock-distributed VM, these are executed as real distribution barriers and can enable shard-local joins.
- In a **multi-root** `from`, there is no single implicit stream; therefore top-level tail clauses are intentionally rejected as ambiguous.
  - Use `for <root> { ... }` blocks after the optional `links` clause to attach tail stages to a specific root stream.
  - If you need a single globally ordered/paginated collection in a multi-root query, compute it via a nested single-root `select` (i.e., make the stream explicit).

### 8.1.1 `for <root> { ... }` blocks (post-LINKS per-root tail disambiguation)

To disambiguate tail stages in multi-root queries, the language supports `for` blocks:

```yed
select {
  users: users@u[*].id,
  books: books@b[*].id,
}
from users@u:User, books@b:Book
links (users)->[writes@w:UserWritesBook]->(books)
for users { order by u.id asc range ..10 }
for books { where b.genre == "Tech" order by b.id desc range 0..5 }
```

Semantics:
- `for <root> { ... }` targets a **FROM root label** (the collection label before `@`).
- The block may contain only tail modifiers: `where`, `order by`, and `range`.
- The block applies to that root's stream **after** link traversal (i.e. post-LINKS), and exists specifically to make multi-root tail stages unambiguous.

If you intended to filter/order a root **before** link traversal (base scan shaping), use parenthesized FROM modifiers instead:

```yed
from (users@u:User order by u.id asc range ..10),
     (books@b:Book where b.genre == "Tech" order by b.id desc range 0..5)
```

Rules:
- Each root may be targeted at most once.
- `for <root> { ... }` blocks must appear after the optional `links` clause.
- Inside a `for <root> { ... }` block, expressions are **root-local**: they may reference the targeted root’s binders (and any `links`-materialized binders reachable from that root), but must not reference *other* roots’ item binders. This prevents accidentally “smuggling” implicit join semantics into multi-root queries.
- Parenthesized FROM modifiers (base scan shaping) and `for <root> { ... }` modifiers (post-LINKS tail) may both be present; they apply at different phases.

### 8.2 Ordering nested arrays (expression-level)

If you want ordering inside a nested array, do it in the projection using array selectors (exact syntax may vary by final parser):

```yed
select users@u[*].{
  id,
  books: u.writes@w[*].books@b[**][order by b.published_date desc],
}
from users@u:User
links (users)->[writes@w:UserWritesBook]->(books@b:Book)
```

---

## 9) Scenario checklist (what the current syntax covers)

Covered well today:
- Single-root scans with filters.
- Multi-root scans (independent roots) + `links` anchored at any root.
- Correlated 1-hop / N-hop traversals, including `<-` and `<->`.
- Traversal-local filtering at base node, edge segment, and later nodes.
- Materializing nested arrays and navigating them in projection.
- Continuation from an intermediate by using a later `links` clause.
- Grouping roots into groups and filtering groups (having).
- Global ordering/pagination; nested ordering via array selectors.

---

## 10) Scenarios not yet fully addressed (and suggested approaches)

### 10.1 Ergonomic fan-out branching from an intermediate

Problem:
- You want: write `users -> foods` once, then branch `foods -> ingredients` and `foods -> restaurants`.

Current workaround:
- Duplicate the prefix per branch (verbose but explicit).

Suggested minimal addition:
- Add an inline fork block at any node position:

```yed
links (users)
  -> [likes@l:UserLikesFood]
  -> (foods@f:Food)
  {
    -> [has_ingredient@hi:FoodHasIngredient] -> (ingredients@i:Ingredient),
    -> [served_at@sa:FoodServedAt] -> (restaurants@r:Restaurant)
  }
```

### 10.2 Distinct / deduplication

Problem:
- `<->` traversals and multi-path traversals naturally produce duplicates.

Approach (user-controlled):
- Keep the pipeline/traversal semantics “literal” and allow duplicates by default.
- Provide `distinct(array)` / `unique_by(array, key)` as standard functions so users can dedup either:
  - in projection (common), or
  - via an explicit pipeline step if you later add one.

### 10.3 Semi-join patterns (“filter parents by existence of linked children”)

Problem:
- Common query: “users who wrote at least one tech book”.

Approach options:
- Projection-level filter once nested materialization exists:
  - `users@u[where len(u.writes@w[*].books@b[**][where b.genre == "Tech"]) > 0]`
- Or allow pipeline `where` to reference materialized nested fields after `links`.

### 10.4 Re-rooting / flattening the unit of query

Problem:
- Sometimes you want the unit of the whole query to be the reached nodes (e.g. books), not the original `users`.

Approach:
- Keep the rule “projection determines shape”; re-root by projecting the flattened collection:
  - `select users@u[*].writes@w[*].books@b[**][**]`.

Why the final `[**]`?
- `users@u[*]` produces one value per user.
- Each per-user value `u.writes@w[*].books@b[**]` is already an `Array<Book>`.
- Therefore the overall result is `Array<Array<Book>>` unless you flatten one more level.

### 10.5 Variable-length / recursive traversals

Problem:
- Graph workloads often need `friends^k`, `reachable`, shortest paths, etc.

Approach:
- If you want it, introduce an explicit repeat operator with tight constraints:
  - e.g. `-> ... -> (friends@f:User){1..3}` or a `repeat { ... }` block.
- Keep it out of the core language if you want to stay minimal; it’s a large semantic surface.

### 10.6 Sharing intermediates without new syntax

If you don’t add fork/let/aliasing, the language can still be complete via:
- duplication + optimizer deduplication, and/or
- doing downstream filtering in projection using already-materialized nested fields.

---

## 11) Non-normative execution note

Even though `links` is defined as nested materialization, a runtime can:
- stream the anchor collection,
- compute nested segments lazily per parent,
- reorder internal joins,
- batch edge lookups,

as long as the externally observed result matches the semantics.
