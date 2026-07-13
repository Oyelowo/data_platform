# Nested Array Navigation (Selectors, Binding, Flattening)

Status: proposal-level semantics.

This document is dedicated to **navigating nested arrays** produced by scans (`from`), traversals (`links`), and expression-level construction.

Related:
- `src/syntax/semantics.md` — overall expression model.
- `src/syntax/pipelining.md` — pipeline clauses and examples.
- `src/syntax/name-resolution-and-scoping.md` — identity rules for pipeline binders.

---

## 1) Two kinds of shapes: one vs many

Think in terms of value shape:

- **one**: a single scalar/object, e.g. `User`, `Book`, `{...}`, `Int`.
- **many**: an array, e.g. `Array<User>`, `Array<Array<Book>>`.

Most navigation problems reduce to: “am I holding **one** or **many** right now?”

---

## 2) The bracket selectors

Selectors live in `[...]` and require a **many** value.

### 2.1 `[*]` — map/iterate (one level)

- Input: `Array<T>`
- Output: `Array<U>` (depends on what you do after it)

Example:

```yed
users@u[*].{ id: u.id }
```

### 2.2 `[where ...]` — filter

- Input/Output: `Array<T>`

Example:

```yed
users@u[where u.age > 18]
```

### 2.3 `[**]` / `[***]` / ... — flat_map (flatten + enter element context)

- `[**]` flat_maps one level: `Array<Array<T>> -> T` (repeated over all inner elements)
- More stars = deeper flat_map: $n$ stars flat_maps depth $n-1$.

Examples:

```yed
nested_books@b[**].id   // Array<Id>
very_nested@x[****].id  // flat_map 3 levels, then access field
```

---

## 3) Binding: `expr@x[...]`

A binder introduced by `@x` names the **current element** while the selector runs and for the remainder of the path chain.

Key intuition:
- The binder “follows the array” in the chain.
- You typically introduce a binder immediately before `[*]`, `[where]`, or `[**]`.

Example:

```yed
users@u[*].{ name: u.name }
```

- `users` is the whole array.
- `u` is a single user inside the mapping.

---

## 4) Member access & projection rules (with arrays)

There are three distinct situations:

1) **Single object**: member access/projection works normally.

```yed
info.name
info.{ name, another: 23 }
info@i.{ id: i.info_id, name, another: 23 }
```

2) **Array value**: you cannot access fields directly.

```yed
users.name        // INVALID
users.{ id }      // INVALID
```

3) **Iterating accessor** (`[*]`) or **flat_map accessor** (`[**]`, `[***]`, ...): member access/projection maps over elements.

```yed
users[*].id          // => Array<Id>   e.g. [1, 2, 3]
users[*].{ id }      // => Array<{id}> e.g. [{id: 1}, {id: 2}, ...]

users@u[*].id        // same as users[*].id, but with explicit binder
users@u[*].{ id: u.id }
```

For nested arrays:

```yed
users@u[*].writes@w[**].date   // flat_map writes across all users, then access date
```

---

## 5) Worked examples

### 5.1 Why `users@u[*].writes` is nested

Assume `links` materializes per-user nested arrays:

```yed
from users@u:User
links (users)->[writes@w:UserWriteBook]->(books@b:Book)
```

Then:
- `u.writes` is `Array<UserWriteBook>` (per user)
- `users@u[*].writes` is `Array<Array<UserWriteBook>>` (per user → array)

### 5.2 Traverse to books along the same path

Canonical “flat_map + dot + flat_map” pattern:

```yed
users@u[*].writes@w[**].books@b[**].{ id: b.id }
```

Shape reasoning:
- `users@u[*].writes` is `Array<Array<Edge>>`
- `writes@w[**]` flat_maps to `Edge` (repeated across all users' writes)
- `.books` yields `Array<Book>` (per edge), so overall it is `Array<Array<Book>>`
- `books@b[**]` flat_maps to `Book`

### 5.3 If you want per-user books (no cross-user flatten)

```yed
users@u[*].{
  id: u.id,
  books: u.writes[*].books@b[**].{ id: b.id },
}
```

Here:
- `u.writes[*]` enters edge element context, so `.books` is valid.
- `books@b[**]` flattens across edges for that single user.

### 5.4 Two-stage flattening (common)

Often you need to flatten at two different “cuts”:

- flatten per-user edges (across users)
- flatten per-edge books (across edges)

That is why `[**]` appears twice in patterns like:

```yed
users@u[*].writes@w[**].books@b[**][**]
```

The final `[**]` is only needed if you intentionally want to flatten across the outermost mapping (users).

---

## 6) Common pitfalls

- `arr.field` is invalid (arrays must be entered with `[*]` / `[**]` before `.`).
- If you see `Array<Array<T>>` in your head, you usually need `[**]` (flat_map) before using `.`.

---

## 7) Links mental model summary

- `links` materializes nested arrays at the level where labels are written.
- Your projection decides whether you keep nesting, flatten, or re-root.
