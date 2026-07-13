
## SELECT examples (LINKS)

This file is example-driven. For the formal semantics model (including strict locality and per-parent gating), see:
- `src/syntax/semantics.md`
- `src/syntax/pipelining.md`

Surface contract reminder:
- `select` is an expression form, but it still requires `from`.
- The source of the query comes from `from`; there is no rootless `select` surface syntax.

### **1. Core Source-Target Relationship**

#### **Original Data Representation**

##### **Nodes Table**

**Users Table**
| id | name | age |  
|--------|----------|-----|  
| User:1 | Alice | 25 |  
| User:2 | Bob | 35 |

**Books Table**
| id | title | genre |  
|----------|----------------------|------------|  
| Book:1 | "Graph Theory 101" | "Science" |  
| Book:2 | "Advanced SQL" | "Tech" |  
| Book:3 | "Introduction to AI" | "Tech" |

##### **Edges Table**

**UserWritesBook**  
| from | to | published_date |  
|------------|------------|----------------|  
| User:1 | Book:1 | 2021-01-01 |  
| User:1 | Book:3 | 2022-01-01 |  
| User:2 | Book:2 | 2019-01-01 |  
| User:2 | Book:3 | 2020-01-01 |

---

#### **Traversal: Collecting Data**

1. **Start with `Users` Table**:  
   This is the **source**.

   ```json
   [
     { "id": "User:1", "name": "Alice", "age": 25 },
     { "id": "User:2", "name": "Bob", "age": 35 }
   ]
   ```

2. **Traverse `UserWritesBook` Edge Table**:  
   This links **Users** to **Books**.

   ```json
   [
     { "from": "User:1", "to": "Book:1", "published_date": "2021-01-01" },
     { "from": "User:1", "to": "Book:3", "published_date": "2022-01-01" },
     { "from": "User:2", "to": "Book:2", "published_date": "2019-01-01" },
     { "from": "User:2", "to": "Book:3", "published_date": "2020-01-01" }
   ]
   ```

3. **Fetch Data from `Books` Table**:  
   Using the `to` field from the edge table, fetch **Books** data.
   ```json
   [
     { "id": "Book:1", "title": "Graph Theory 101", "genre": "Science" },
     { "id": "Book:2", "title": "Advanced SQL", "genre": "Tech" },
     { "id": "Book:3", "title": "Introduction to AI", "genre": "Tech" }
   ]
   ```

---

### **2. Assembling Intermediate Results**

#### How to read the query (semantics)

The intent is to keep the surface syntax literal:

- `label` names the whole array value in scope (e.g. `users` is the array; `len(users)` is valid)
- `@binding` names the current element when iterating/filtering/projecting (e.g. `@u` is a single user)
- `links` does not primarily mean “produce tuples”; it declares traversals that attach **virtual nested fields** onto upstream items

Core rule reminder:
- The projection (`select <expr>`) determines the result value/shape.
- There is no implicit row-by-row mapping from `from`: per-element results require explicit selectors like `[*]` / `[where ...]` / `[n]` / `[**]`.
- `select 1 from users@u:User` returns `1`, not an array or repeated per-user scalar.
- `u.best_friend?.id` remains an option-like projection expression; later `.is_none()` / `.is_some_and(...)` stay ordinary option-like method calls on that result.
- For query/path collections, `[n]` and slices are always semantically valid. `[order by ...][0]` uses explicit path order; bare unordered `[0]` uses arbitrary engine-chosen order and is intentionally nondeterministic.

So if you write:

```py
from (users@uf:User)
links (users)->[writes@wl:UserWritesBook]->(books@b:Book)
```

you can later navigate as if `writes` and `books` are nested fields:

- `u.writes@w[*]` iterates the edge results associated with user `u`
- `w.books@b[*]` (or `...[**]`) iterates the node results associated with edge `w`

This is a logical meaning only; an execution engine can still stream/async-evaluate per top-level item and per nested collection.

Implementation note:
- The compiler/runtime may implement `links` either as a traversal operator or by lowering it into an explicit join-chain substrate (joins + per-parent nest materialization). This is an execution strategy choice and must not change the observed result.
- The optimizer/physical planner may also insert internal `Exchange` barriers (broadcast/shuffle/gather/merge). These are not user-visible except via performance characteristics.

For **User:1** (Alice):

- Alice has written **Book:1** ("Graph Theory 101") and **Book:3** ("Introduction to AI").

```py
select users@u[*].{
    id,
    name,
    age,
    books_written: u.writes@w[*].books@b[**].{
        book_id: b.id,
        title,
        genre,
        published_date: w.published_date
    }
}
from (users@uf:User)
links (users)->[writes@wl:UserWritesBook]->(books@b:Book)
```

Notes:

- `books@b[**]` flattens one level, so the result is a single array of books even if `u.writes[*].books[*]` would otherwise produce nested arrays.
- flattening can be deeper if needed. rule: $n$ stars means flatten depth $n - 1$.
  - `books@b[***]` is like `.flat(2)`
  - `books@b[****]` is like `.flat(3)`
- Implementation note: selector flattening is also a planning signal; physical lowering can upgrade the streaming unit to leaf-granularity when `[**]` appears in the final projection.
- a label used in `links` should not collide with an existing schema field name on the same object (recommended rule: error unless renamed).

Common pitfall:

- if you write `u.writes@w[*].books@b[*]` you now have a nested value `[[Book]]`.
- `u.writes@w[*].books@b[*].id` is invalid because `.id` would be applied to `[[Book]]` (a nested array), not a single `Book`.
- use `[**]` when you want a flat list: `u.writes@w[*].books@b[**].id`.
- or keep nesting by projecting per element: `u.writes@w[*].books@b[*].{ id: b.id }`.

Deeper chained traversals follow the same rule. If you want to keep traversing through multiple
link-attached collections without preserving intermediate nested arrays, flatten at each hop that would
otherwise leave the current value nested.

Example flat chain:

```yed
u.writes@w[where w.date > dt'2024-02-01'][order by w.date asc]
  .books@b[**]
  .read_by@r[**]
  .readers@ru[**]
  .follows@f[**]
  .other_users@ou[**]
```

Using `[*]` at one of those stages is still legal if you intentionally want to preserve that extra nesting,
but then the next field access must respect the nested shape rather than pretending it is already flat.


and links can even be more complex:


```py
from users@u:User 
links (users where u.age > 50)->[writes@w:UserWritesBlog where w.date > u.dob]->(blogs@b:Blog where b.title == 'cool'), 
  (users)->[befriends@bf:UserBefriendsUser]->(follower:User)
```

Mixed-direction chains are also valid in one path when that is the intended traversal:

```yed
from users@u:User
links (users)->[writes@w:UserWritesBook]->(books@b:Book)<-[read_by@r:UserReadsBook]<-(readers@ru:User)
```

That is a single links path with multiple segments. Comma-separated entries remain available when you want
multiple independent paths instead.

Scope intuition:

- inside a segment predicate like `[writes@w:... where w.date > u.dob]`, both `w` (current edge) and `u`/`us` (upstream bindings) are in scope
- outside iteration contexts, use the plural labels (`users`, `blogs`, ...) for whole-array operations like `len(users)`
- after `links`, outer `where` / `group by` / `order by` remain root-local; raw link binders do not leak, so `where b.id > 0` is invalid and must instead be written as a segment-local predicate, a root-anchored reduction like `u.writes@w[*].books@b[**].any(|b| b.id > 0)`, or an explicit nested query
- `links inner` is clause-wide today; it applies the required-match contract to every lowered segment in that links path. Mixed optional/required chains are therefore not a current surface form and should not be treated as an unimplemented QIR lowering lane.

---

### **3. Filtering with higher-order predicates (`any` / `all`)**

It’s common to express existence checks over a correlated subquery using `.any(|row| ...)`.
This reads as an existential predicate and can reference both the closure parameter and outer bindings.

Implementation status note:

- Semantically, `.any(|x| ...)` / `.all(|x| ...)` are first-class collection predicates.
- They are executable in the VM for in-memory arrays (including arrays materialized by `links`).
  In query contexts, the optimizer may also lower some forms (especially subquery receivers) into
  Semi/Anti/Mark/Nest joins; this is an execution strategy choice and must not change results.

The closure parameter is the collection’s element type. In the example below, `u.writes@w[*].books@b[**]` is an array of `Book` objects, so `book` is a `Book`. If you instead build an array of scalars (e.g. `...books@b[**].id`), the closure parameter is that scalar.

Example: users that have written any book in the Tech genre:

```yed
select users@u[*].id
from users@u:User
links (users)->[writes@w:UserWritesBook]->(books@b:Book)
where u.writes@w[*].books@b[**].any(|book| book.genre == "Tech")
```

Selector-based equivalent:

```yed
where !is_empty(u.writes@w[*].books@b[**][where b.genre == "Tech"])
```





```json
{
  "id": "User:1",
  "name": "Alice",
  "age": 25,
  "books_written": [
    {
      "book_id": "Book:1",
      "title": "Graph Theory 101",
      "genre": "Science",
      "published_date": "2021-01-01"
    },
    {
      "book_id": "Book:3",
      "title": "Introduction to AI",
      "genre": "Tech",
      "published_date": "2022-01-01"
    }
  ]
}
```

For **User:2** (Bob):

- Bob has written **Book:2** ("Advanced SQL") and **Book:3** ("Introduction to AI").

```json
{
  "id": "User:2",
  "name": "Bob",
  "age": 35,
  "books_written": [
    {
      "book_id": "Book:2",
      "title": "Advanced SQL",
      "genre": "Tech",
      "published_date": "2019-01-01"
    },
    {
      "book_id": "Book:3",
      "title": "Introduction to AI",

  ---

  ### **4. Join-like patterns (without implicit JOIN)**

  YED does not have a general-purpose SQL-style `JOIN` surface construct.

  Instead:

  - Use `links` to express **graph/document relationships** (correlated per-parent traversal that materializes nested arrays).
  - Use nested single-root `select` expressions to make any required **cross-entity correlation** explicit.
  - In multi-root `from`, treat roots as **independent facets**; there is no implicit cross-product stream.

  #### 4.1 Flat “pairs” from a traversal (explicit flattening)

  If you want a flat list of `(user_id, book_id)` pairs from a relationship, project it explicitly and flatten at the right level:

  ```yed
  select users@u[*].writes@w[**].books@b[**].{ user_id: u.id, book_id: b.id }
  from users@u:User
  links (users)->[writes@w:UserWritesBook]->(books@b:Book)
  ```

  #### 4.2 Filter parents by existence of related children (semi-join style)

  ```yed
  select users@u[*].id
  from users@u:User
  links (users)->[writes@w:UserWritesBook]->(books@b:Book)
  where u.writes@w[*].books@b[**].any(|book| book.genre == "Tech")
  ```

  #### 4.3 Multi-root facets + `for <root> { ... }` (tail disambiguation)

  When you have multiple `from` roots, roots are independent. Tail stages like `where/order/range` are ambiguous at top-level, so use per-root `for` blocks:

  ```yed
  select {
    users: users@u[*].id,
    books: books@b[*].id,
  }
  from users@u:User, books@b:Book
  links (users)->[writes@w:UserWritesBook]->(books)
  for users { order by u.id asc range ..10 }
  for books { where b.genre == "Tech" order by b.id desc range ..5 }
  ```

  Rule reminder:

  - `for <root> { ... }` is **root-local** (no implicit join stream): expressions inside a root’s `for` block must not reference other roots’ *item* binders.

      "genre": "Tech",
      "published_date": "2020-01-01"
    }
  ]
}
```
