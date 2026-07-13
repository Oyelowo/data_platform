
Okay, let's synthesize everything we've discussed into a comprehensive description of your query language's syntax, semantics, and functionality, suitable for explaining it to another LLM or developer. We'll highlight the key design choices and how they differ from traditional relational models.

---

**Query Language Specification: A Document/Graph-Oriented Approach**

**1. Core Philosophy & Data Model**

*   **Paradigm:** Collection-oriented, operating on sets of documents (conceptually similar to JSON objects or graph nodes/edges). It avoids the strict row-by-row processing model of traditional SQL.
*   **Documents/Nodes:** Flexible JSON-like structures stored in named collections (e.g., `User`, `Post`, `Blog`). Analogous to tables or graph node types. Fields can contain primitives, nested objects, arrays, or special Link types.
*   **Relationships/Links:** Modeled explicitly in the schema using:
    *   **Edge Collections:** Separate collections (e.g., `UserWritesBlog`) containing `_from` and `_to` fields storing `RecordId`s, plus edge properties. This is the preferred way for relationships with properties.
    *   **Link Fields:** Fields within a document schema defined with types `LinkOne<TargetTable>` or `LinkMany<TargetTable>`. These store `RecordId`(s) referencing documents in the `TargetTable`.
*   **Record IDs:** Universal identifiers with the format `TableName:Value` (e.g., `User:uuid-123`, `Post:99`). The `Value` part's type is typically defined by the target table's primary key schema. This structure allows direct fetching and implies the type of the referenced document.

**2. Query Structure (Focus on SELECT)**

The primary query structure follows this pattern (keywords are written in lowercase):

```
[let <var> = <expr>; ...]                 // optional variable definitions
select <projection-expression>            // defines the output value (evaluated at the end)
from <from-clause>                        // defines initial collections and scope
[links <link-path>, ...]                  // declares traversals that attach nested results
[where <global-filter-expr>]              // filters after from/links (logical)
[group by { <key_name>: <key-expr>, ... } into <groups_name>,
         { <key_name>: <key-expr>, ... } into <groups_name2>,
         ...]
[order by <order-expr> asc|desc, ...]     // orders after grouping/filtering
[start <start-expr>]                      // offset after ordering
[limit <limit-expr>]                      // limit after ordering/start
[;]                                       // optional terminator
```

**3. Key Clause Semantics**

*   **`from (label@item_binding:collection where <inline_filter>)`**
    *   Establishes the initial scope. Fetches documents from `Collection`.
    *   `label` (optional): refers to the *entire collection* (`Array<Object(CollectionType)>`).
    *   `@item_binding` (optional): refers to *each item* (`Object(CollectionType)`) during iteration (inline `where`, `links`, iterating projections like `label@item_binding[*].{...}`).
    *   `where <inline_filter>`: applied immediately to the base `collection`, filtering items *before* linking. can use `@item_binding`.
    *   Multiple `from` sources imply an initial **cross product**.
        *   Multiple `from` sources introduce multiple **independent root collections** in scope (no implicit cross product).

*   **`links <path>, <path>, ...`**
    *   Declares traversals that **materialize nested results** associated with upstream items.
    *   This is intentionally *not* specified as a relational tuple join. Instead, `links` defines how downstream path expressions like `u.writes@w[*].books@b[**]...` are evaluated.
    *   **Path segment (common): `(node_spec) -> [edge_spec] -> (node_spec)`** (and `<-`)
    *   **Path segment (attachment): `(node_spec) -> (node_spec)`**
        *   `node_spec`: `(label@bind:type where filter)` or `(label)` (reference).
        *   `edge_spec`: `[label@bind:type where filter]` or `[label]`.
        *   **Semantics (logical):** for each upstream element, traverse the edge collection and attach matched edges/nodes under the chosen segment labels.
        *   **Filtering:** inline `where` inside `()` or `[]` filters nodes/edges during traversal. these predicates may reference upstream bindings (e.g. `uf`, earlier segment bindings) and the current segment binding.
        *   **Scope:**
            *   the **label** (e.g. `users`, `writes`, `books`) names the *whole nested array* result and can be used for aggregation (`len(users)`), slicing, etc.
            *   the **binding** after `@` (e.g. `uf`, `wl`, `b`) names the *current element* when iterating/projecting/filtering.
    *   **No implicit joins:** `links` is not a tuple-join model. Edge-to-edge hops are disallowed.
    *   **Union (`|`):** supported within `()` (nodes) and `[]` (edges).
        *   Preferred (single label/binder, union type):
            *   `links (users)->[actions@e:(UserWriteBook | UserReadBook)]->(books@b:Book)`
            *   `links (people@p:(User | Admin)) -> (accounts@a:Account)`
        *   If you need different labels/binders per alternative, write separate comma-separated paths instead of a single union segment.

Example (why union for nodes can be useful):

```yed
from repos@r:Repo
links (repos)->(owners@o:(User | Org))

select repos@r[*].{
    repo_id: r.id,
    repo_name: r.name,
    owner_id: o.id,
    owner_name: o.name,
}
```

This avoids duplicating the same traversal twice (one for `User`, one for `Org`) when you only need fields common to both variants.

Note: accessing a common field like `o.id` does not tell you *which variant* (`User` vs `Org`) you got. If you need variant-specific behavior/fields, you must project a discriminator (a real field like `o.kind`) or use an explicit type-test/match construct (if/when supported).

*   **`where <global-filter-expr>`**
    *   Filters after `from`/`links`.
    *   Can reference any label/binding introduced by `from`/`links`.

*   **`group by { <key_name>: <key_expr>, ... } into <groups_name>, ...`**
    *   A single `group by` clause may contain multiple comma-separated group specifications.
    *   Each group specification must include an `into <name>` (so multiple group results can coexist).
    *   **Normal grouping semantics (final intent): produces group objects.**
        *   `group by` transforms the current root stream of items into a stream of **groups**.
        *   `into <groups_name>` binds the resulting groups collection.
        *   Each group item `g` exposes:
            *   the grouping key values as fields, and
            *   the grouped members via the root labels from the `from` clause.
                *   Example: `from users@u:User` implies each group exposes `g.users` (an array of `User`).
                *   Example: `from weathers@w:Weather` implies each group exposes `g.weathers` (an array of `Weather`).
            *   There is intentionally **no** universal `g.items` field; member access is always via the meaningful root label(s).
        *   Keys are always named via object fields (e.g. `{ city: u.city, decade: (u.age/10)*10 }`).
        *   Key expressions must yield **one scalar value per root item**.
            *   If a key yields a collection/object (or conceptually yields many values per root item), it is a type error.
            *   To group based on linked/nested values, compute a scalar summary (e.g. `count(u.blogs@b[*])`, `max(u.blogs@b[*].views)`, `u.blogs@b[*].any(|b| b.tag == "x")`), or re-root via a subquery/flatten.
    *   **Do we need `having`?**
        *   Not necessarily.
        *   Instead of a dedicated `having` keyword, you can filter groups using normal filtering on the groups collection:
            *   `select groups@g[where <predicate over g>].{...} from users@u:User group by { city: u.city } into groups`
        *   Conceptually this is the same as SQL `having`, but it stays within one consistent `where` mechanism.

*   **`order by`, `start`, `limit` (global)**
    *   Standard ordering and pagination applied to the collection before the final projection.

*   **`select <projection_expr>`**
    *   **Final step:** evaluated once after all preceding clauses.
    *   **Scope:** uses the scope defined by the preceding clauses.
        *   If `group by` was used, the groups collection (`groups` by default, or the name provided via `into`) is available.
        *   Otherwise, labels/bindings from `from`/`links` are available.
    *   **Output:** the result of the entire query is the value produced by evaluating `<projection_expr>`. it is not implicitly “one row per input row” unless the projection explicitly iterates (e.g. `users@u[*].{...}`).

**4. Expression Semantics & Accessors**

*   **Collections First-Class:** Variables can hold arrays of objects.
*   **Explicit iteration:** to operate on elements within an array (`Array<T>`), explicit iteration/mapping syntax is required:
    *   `array[*]`: Represents iterating all elements (conceptually yields `Array<T>`).
    *   `array[where cond]`: Filters elements (yields `Array<T>`).
    *   `array[start..end]`: Slices elements (yields `Array<T>`).
    *   `array[order by ...]`: Sorts elements (yields `Array<T>`).
    *   `array[**]`: Flattens one level (`Array<Array<T>>` -> `Array<T>`).
    *   `array[***]`, `array[****]`, ...: Flattens multiple levels.
        *   rule: $n$ stars means flatten depth $n - 1$ (like `.flat(2)`, `.flat(3)`, ...).
*   **Item Access:**
    *   `array[index]`: Accesses a single element by integer index (yields `T` or `Nullable<T>`).
*   **Member Access (`.`):**
    *   `object.field`: Accesses a field on a single object (yields `FieldType` or `Nullable`).
    *   `array.field`: **INVALID**. Must use `array[*].field` etc.
    *   `iterating_array_accessor.field` (e.g., `users[*].name`): **VALID**. Implicitly maps the field access over the elements, returning `Array<FieldType>`.
*   **Document Access (`.{...}`):**
    *   `object.{...}`: Creates a new object based on projecting from a single source object.
    *   `array.{...}`: **INVALID**.
    *   `iterating_array_accessor.{...}` (e.g., `users[*].{name: u.name}`): **VALID**. Implicitly maps the projection over the elements, returning `Array<ProjectedObject>`.
*   **`@alias`:** `collection_expr @ alias accessor` binds the name `alias` to *each item* being processed by the `accessor` (`[*]`, `[where]`, `.{...}`, etc.). The alias has the *element type* (`T` if `collection_expr` is `Array<T>`) and is available within the accessor's context (e.g., inside the `where` expression or the `{...}` projection).
*   **Null Safety (`?.`, `?[`, `?{`):** Optional chaining. If the base expression evaluates to `Null`, the access short-circuits and returns `Null`. The result type becomes `Nullable<T>`.
*   **Links:** Accessing `LinkOne<T>` field (`u.author`) implicitly dereferences to `Nullable<Object(T)>`. Accessing `LinkMany<T>` field (`u.posts`) implicitly dereferences to `Array<Object(T)>`. Accessing `.id` on a link field (`u.author.id`) returns the `RecordId(T)`.

**5. Type System**

*   **Static Typing:** Aims for strong static typing, leveraging schema definitions (`@table`, `type`) and explicit type annotations (`let x: Int = ...`).
*   **Type Inference:** Infers types for expressions where not explicitly annotated.
*   **Core Types:** `Int`, `Float`, `String`, `Boolean`, `DateTime`, `Duration`, `Uuid`, `Bytes`, `Null`, `Any`, `Object`, `Array`, `RecordId`, `LinkOne`, `LinkMany`, `Union`, `Nullable`, `Function`.
*   **Checks:** Performed during logical planning to catch errors early (operator misuse, undefined fields/variables, incorrect function arguments, invalid assignments/assertions).

This detailed description should provide a solid foundation for understanding your language's intended behavior and how it contrasts with traditional relational query languages.

---

## Execution note (non-normative)

The semantics above define the **logical** meaning of queries (what value they denote). They do not require full materialization.

- A runtime can stream top-level results item-by-item.
- Nested arrays introduced by `links` can be produced lazily (iterator/stream per parent item).
- `[**]` (flatten) only changes the *observable shape/order* of results; it does not force eager evaluation.
