# Name Resolution & Scoping (Query Semantics)

This document defines **query-specific name resolution and scoping** rules.

Goals:
- Predictable identity: the same spelling does not silently change meaning.
- Orthogonal concepts: “where data comes from” is separate from “how we name the current element”.
- Make ambiguity impossible (or a compile error), especially with multi-path `links`.

---

## 1) Two kinds of query names

Within a single query statement there are two distinct name classes.

### 1.1 Collection labels (pre-`@`)
Examples: `users`, `foods`, `writes`, `groups`.

Meaning: a **collection value** (typically `Array<...>`) produced by `from`, `links`, grouping, or other pipeline operations.

In projection, you primarily work with collection labels + selectors:

```yed
select users[*].id
from users:User;
```

### 1.2 Item binders (post-`@`)
Examples: `@u`, `@f`, `@w`, `@b`, `@g`.

Meaning: a **name for the current element** of some collection, used in closure-like contexts:
- scan predicates (`where u.age > 20`)
- `links` segment predicates
- array selectors in expressions (`users@u[where ...]`, `users@u[*].{...}`)

---

## 2) Orthogonality: collection introduction vs binder naming

There are two independent concepts:

### 2.1 Collection introduction (data source / produced stream)
This answers: “where does this collection value come from?”.

Collection introduction happens in **typed header forms**:
- `name:Type`
- `name@binder:Type`

Examples:

```yed
from users:User
from users@u:User
```

### 2.2 Binder naming (element name)
This answers: “what do we call the current element of a collection in this scope?”.

Pipeline binder identities are introduced only by **typed definers**:
- `name@binder:Type`

Untyped pipeline references never include `@binder`.

Inside expressions (outside the pipeline header syntax), `@binder` may still be introduced selector-locally (see §6).

---

## 3) The identity rule (pipeline)

Within a single statement, pipeline binders behave like **single-assignment names**:

- A binder identity (e.g. `@u`) may be **introduced at most once** in the statement’s pipeline namespace.
- After introduction, `u` may be **referenced any number of times**, and it always refers to the same identity.
- Any attempt to make `@u` mean a second, different identity is a compile error.

The same policy applies to collection labels (pre-`@`): a collection label may be introduced once and referenced many times.

---

## 4) Header forms: reference vs introduction

In pipeline header positions (including `from`, `links` node/edge specs, mutation headers, `group by { city: u.city } into groups`):

### 4.1 `name:Type`
- Introduces a **collection label** `name`.
- Does not introduce an item binder.

### 4.2 `name@binder:Type`
- Introduces a **collection label** `name`.
- Introduces the **binder identity** `binder`.

### 4.3 `name` (no `:Type`)
- References an existing collection label.
- Does not introduce a new collection.

Rule of thumb:
- In pipeline headers, you will typically see each binder spelling `@u` **exactly once** at its introduction site (`name@u:Type`).
- After that, headers use only collection labels (`name`), while predicates/expressions use `u`.

---

## 5) Projection vs pipeline clauses

### 5.1 Projection is collection-oriented
Projection should primarily use collection labels + selectors:

```yed
select users[*].id
from users@u:User;
```

Item binders (`u`) are meant for predicate/selector contexts, not as standalone root projection values.

### 5.2 Pipeline clauses may reference binders
`where`, `group by`, `order by`, and `links` segment predicates may reference in-scope binders.

---

## 6) Selector-local binders inside expressions

Inside expressions, `@x` may introduce a **selector-local binder**:

```yed
select users@u[*].{ id: u.id }
from users:User;
```

These binders are lexically scoped to the expression chain.

Shadowing inside expressions may be allowed, but is often confusing. Prefer a lint/warn:

```yed
select users@u[*].{ titles: u.writes@u[*].books@b[**].title }
```

The inner `@u` here is a selector-local binder; it is not a pipeline rebinding.

---

## 7) LINKS semantics: anchoring, naming, and ambiguity

### 7.1 Anchor rule: no new data source at the start
In a SELECT `links` clause, the first node of each path is an **anchor**.

The anchor must **reference an existing in-scope collection label**; it must not introduce a new collection data source.

Allowed:

```yed
from users@u:User
links (users) -> [writes@w:UserWritesBook] -> (books@b:Book);
```

Not allowed (typed anchor introduces a new source / redeclares):

```yed
from users@u:User
links (users@u:User) -> [writes@w:UserWritesBook] -> (books@b:Book);
```

Note: anchors and other untyped references are written as `(users)` (no `@binder`).

### 7.2 Segment predicates are filters, not new identities
Adding `where` on a segment does not create a second `u`; it only restricts what flows through that path:

```yed
from users@u:User
links (users where u.age > 50) -> [writes@w:UserWritesBook] -> (books@b:Book);
```

### 7.3 Multi-path links: ordered visibility (left-to-right)
In a SELECT `links` clause, multiple traversal paths may be comma-separated.

Rule:
- Paths are interpreted **left-to-right**.
- Names (collection labels / binders) introduced by earlier paths are **in scope** for later paths.
- Therefore, a later path may anchor from an intermediate label introduced by an earlier path (e.g. `(foods)` after a prior path introduced `foods`).

Constraint:
- A path may only anchor from a label that is already in scope from `from` / earlier pipeline clauses **or** introduced by an earlier sibling path in the same `links` clause.
- Anchoring from a label that is introduced only in a later sibling path is a compile error.

Even with shared visibility, we still enforce two global constraints to avoid accidental collisions:
- **Binder introductions are statement-wide unique** (§3). If one path introduces `@f`, no other path may introduce `@f` again.
- **Materialized nested field labels should not collide**. If two sibling paths both attach a nested field named `foods` onto the same parent items, projection becomes ambiguous (which `u.foods`?). Prefer distinct labels like `liked_foods` vs `eaten_foods`.

### 7.5 Where references can be used
References are usable wherever the grammar expects a **collection label**:
- In a `links` path: you can reuse labels introduced earlier in the *same path*.
- In projection: you typically access nested results by navigating from the parent item (e.g. `u.liked_foods`).

References are usable across sibling comma-separated `links` paths **only** when the referenced label is already in scope by the left-to-right rule (§7.3).

### 7.6 What this model does *not* cover
This model supports continuing from intermediates within a single `links` clause (left-to-right), but it still does not provide a *general* mechanism to name and reuse arbitrary intermediate results with explicit control over evaluation order beyond the comma ordering.

If you need a single shared intermediate with multiple downstream branches *without duplication*, you will need one of:
- an aliasing operator (e.g. `as/into`), or
- a general binding form (e.g. `let`), or
- a qualification mechanism that can refer to a specific path result.

### 7.7 Non-ugly ways to support branching (future options)
If you want branching from an intermediate segment **without** `let` and without `as/into`, there are a few syntax options that preserve a left-to-right “flow”:

Option A: **Inline fan-out (fork) block** (most flow-preserving)

```yed
from users@u:User
links (users)
   -> [likes@l:UserLikesFood]
   -> (foods@f:Food)
   {
     -> [has_ingredient@hi:FoodHasIngredient] -> (ingredients@i:Ingredient),
     -> [served_at@sa:FoodServedAt] -> (restaurants@r:Restaurant)
   };
```

Meaning: the fork block duplicates the current intermediate (`foods@f`) as the starting point for multiple continuations, but it is written once.

Option B: **Qualified continuation** (no aliasing keyword, but introduces a qualifier)

Example idea (syntax not chosen):

```yed
links
  (users) -> [likes@l:UserLikesFood] -> (foods@f:Food),
  (likes.foods where f.name == "rice") -> [served_at@sa:FoodServedAt] -> (restaurants@r:Restaurant);
```

Meaning: `likes.foods` explicitly chooses which previously materialized nested result is being continued.

Option C: **Keep current rules; rely on optimizer**

Allow users to duplicate prefixes per branch, but document that execution may deduplicate shared work.

If you want the language to remain minimal today, Option C is the simplest; if you want the DSL to express branching ergonomically, Option A is usually the cleanest.

This rule removes ambiguity without adding extra syntax.

Example (invalid because it tries to reference `foods@f` from a sibling path):

```yed
from users@u:User
links
  (users where u.name == "Bob") -> [likes:UserLikesFood] -> (foods@f:Food),
  (foods where f.name == "rice");
```

How to express the intent instead:

1) Push the filter into the same path:

```yed
from users@u:User
links
  (users where u.name == "Bob") -> [likes:UserLikesFood] -> (foods@f:Food where f.name == "rice");
```

2) Or filter in projection over the nested results that `links` materializes:

```yed
select users@u[*].{
  ...,
  bob_rice_foods: u.likes@l[*].foods@f[**][where f.name == "rice"],
}
from users@u:User
links (users where u.name == "Bob") -> [likes@l:UserLikesFood] -> (foods@f:Food);
```

### 7.4 “Branching” from an intermediate segment (without new syntax)
Sometimes you want:

> traverse `users -> foods`, then from those *foods* explore multiple different next hops.

If we forbid cross-path references and don’t add aliasing syntax, the unambiguous way is to **duplicate the prefix per branch** and name outputs distinctly.

Example (two branches from the same logical `foods`):

```yed
from users@u:User
links
  // branch A: foods -> ingredients
  (users) -> [likes@l:UserLikesFood] -> (liked_foods@lf:Food)
         -> [has_ingredient@hi:FoodHasIngredient] -> (ingredients@i:Ingredient),

  // branch B: foods -> restaurants
  (users) -> [likes2@l2:UserLikesFood] -> (liked_foods2@lf2:Food)
         -> [served_at@sa:FoodServedAt] -> (restaurants@r:Restaurant);
```

Tradeoff:
- This may repeat work logically, but an execution engine can still optimize/deduplicate.
- The semantics remain explicit: the two branches produce two distinct nested fields (`liked_foods` vs `liked_foods2`).

If you truly need one shared intermediate collection that multiple branches reference, you need *some* handle for it (either a `let`-style binding or an explicit aliasing operator). Without a handle, there is nothing to reference.

---

## 8) Mutations (CREATE/UPDATE/UPSERT/DELETE/LINK/UNLINK)

Mutations follow the same identity rule:
- pipeline collection labels are introduced once
- pipeline binder identities are introduced once

Good:

```yed
create users@u_created:User [{ id: 1, age: 10 }]
link (users where u_created.id == 1)
  -> [writes@w:UserWritesBook { since: 1 }]
  -> (books@b:Book where b.id == 10);
```

Bad (attempts to introduce `@u` twice with different meanings):

```yed
create users@u:User [{ id: 1, age: 10 }]
link (users@u where u.id == 1)
  -> [writes@w:UserWritesBook { since: 1 }]
  -> (books@b:Book where b.id == 10);
```

---

## 9) Summary (rules you can memorize)

- **Collections** (`users`, `foods`) are values.
- **Binders** (`@u`, `@f`) name “the current element” for predicates/selectors.
- **Typed header** (`:Type`) introduces a collection source/stream.
- **Pipeline binders are introduced only by typed definers** (`name@u:Type`).
- After introduction: headers reference collections only (`(users)`), predicates/expressions use binder names (`u`).
- **Multi-path `links`** do not share names (no cross-path references); each path is self-contained.
- **Link-segment binders are lexical**: `@w` / `@b` are usable inside the segment predicate or inside a root-anchored path/closure that introduces them, but they do not leak into outer tail clauses like `where b.id > 0`.
