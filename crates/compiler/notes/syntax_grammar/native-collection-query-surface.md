# Native Collection and Query Surface

Status: proposed source contract.

This note defines the long-term source surface for native collection/query
operations in Yed. The goal is to keep Yed a general-purpose language:
operations are ordinary expressions, methods, functions, closures, selectors,
and path expressions. QIR may recognize selected standard-library identities
and lower them to native query algebra, but source programs should not become a
SQL-like keyword DSL.

## First principles

1. Methods/functions are canonical.
   Path selectors and pipeline clauses are ergonomic surface forms that lower to
   the same typed operation when possible.

2. Native recognition is by resolved identity.
   QIR must recognize `DefId`/lang-item identity, not user-visible names. A user
   method named `sum`, `enumerate`, or `group_by` is just a user method unless
   resolution proves it is the standard-library native operation.

   This also means broad stdlib modules are not automatically query-native.
   `HashMap`, `BTreeMap`, `HashSet`, `BTreeSet`, heaps, geo/search/time/tree,
   binary-search helpers, decimal/money, and algorithm APIs are ordinary
   language surface first. They participate in QIR only when their resolved
   identity carries a known native contract and the optimizer can preserve
   cardinality, ordering, errors, effects, precision/rounding, distribution,
   and backend capability.

3. `[...]` remains collection/path selector syntax.
   A bracket selector always transforms the current collection value. It must
   not secretly mean graph depth, recursive closure, or unrelated control flow.

4. Pipeline clauses are still useful.
   `from`, `links`, root-local `where`, `group by`, `order by`, and `range`
   define storage-stream placement. Projection expressions can express
   similar transforms over values, but those transforms happen where the value is
   produced unless the optimizer proves a safe pushdown.

5. Query results are values.
   A `select` expression returns the projection expression's value. Per-element
   mapping remains explicit through selectors or methods.

## Projection after collection selectors

Collection-preserving selectors compose directly into field and record
projection. Once a bracket selector has shaped a collection, an additional
`[*]` wildcard is not required just to project each selected item:

```yed
users@u[where u.active][order by u.name].id
users@u[order by u.name][20..30].{ id: u.id, name: u.name }
```

This applies after collection-shaping selectors such as `[where ...]`,
`[order by ...]`, `[start..end]`, `[group by ...]`, `[enumerate]`,
`[distinct]`, and `[distinct by ...]`; each of those forms already keeps the
receiver as a collection of selected elements.

The explicit wildcard spelling remains accepted when the author wants to make
the map/path step visible:

```yed
users@u[where u.active][order by u.name][*].id
users@u[order by u.name][20..30][*].{ id: u.id, name: u.name }
```

These forms have the same source-level meaning. QIR may still choose different
physical shapes, such as pushing order/range into a root stream or a per-left
TopK, when facts prove that doing so preserves value, order, and runtime-error
semantics.

## Rust analogy

Rust generally represents these operations as normal methods:

```rust
xs.iter().filter(|x| pred(x))
xs.iter().map(|x| f(x))
xs.iter().filter_map(|x| maybe(x))
xs.iter().flat_map(|x| many(x))
xs.iter().enumerate()
xs.iter().skip(n).take(m)
xs.sort_by_key(|x| std::cmp::Reverse(x.score))
```

Rust does not have built-in grouping or graph recursion in `Iterator`; those are
usually library functions or explicit loops/data structures. Yed should follow
that spirit: native query support is a library/compiler contract, not a new
sub-language.

Rust uses `(index, item)` for `enumerate()` mostly because tuple products are
the lightweight anonymous product type available everywhere. Yed should not copy
that limitation. Since Yed supports records and must support record
destructuring, native products can expose named fields.

## Pipeline vs expression surface

Pipeline placement:

```yed
select users@u[*].{ id: u.id, name: u.name }
from users@u:User
where u.active
order by u.name asc
range 20..30
```

Value-level expression shaping:

```yed
select users@u[where u.active][order by u.name asc][20..30].{
    id: u.id,
    name: u.name,
}
from users@u:User
```

These are not always equivalent. The pipeline form restricts the root stream
before downstream `links`, grouping, and pagination-sensitive work. The
expression form shapes the already available collection value. QIR may push the
expression-level form into the pipeline only when that preserves semantics.

## Callback conventions

Native collection/query methods should use a small, predictable closure
convention:

```yed
items.map(|item| expr)
items.filter(|item| pred)
items.filter_map(|item| maybe_item)
items.flat_map(|item| many_items)
items.group_by(|item| key)
items.order_by(|item| order_key)
items.any(|item| pred)
items.all(|item| pred)
items.none(|item| pred)
```

When a method adds positional context, the original item stays first and the
extra context follows it:

```yed
items.map_indexed(|item, index| expr)
items.for_each_indexed(|item, index| effect)
```

That keeps the callback family stable: the common value remains the first
parameter, and extra query context is appended. `enumerate()` avoids the
ordering question entirely by returning a structural value:

```yed
items.enumerate().map(|entry| {
    index: entry.index,
    value: entry.value,
})
```

For binary relation APIs, use left-to-right data order:

```yed
users.join_by(posts, |user, post| user.id == post.author_id)
```

Closure arity and native meaning are determined by the resolved standard-library
method identity, not by parser magic.

## Arguments and configuration objects

Do not add named arguments as a separate call mechanism for the query surface.
Use ordinary positional arguments for small APIs and object/config values for
multi-field or extensible APIs.

Small positional APIs:

```yed
items.slice(20..30)
items.join_by(other, |left, right| left.id == right.owner_id)
items.order_by(|item| order::desc(item.score))
```

Extensible configuration APIs:

```yed
graph::transitive_closure({
    seed: users@u[where u.id == start_id][*],
    step: |user| user.friends@friend[*],
    key: |user| user.id,
})
```

This keeps configuration as an ordinary value that can be stored, passed,
validated, inferred, and typed like every other expression. A future call-site
sugar can desugar to a config object, but QIR and type checking should not
depend on a special named-argument subsystem.

## Patterns, destructuring, and reassignment

Records returned by native methods should be destructurable anywhere patterns
are accepted: `let`, closure parameters, `for`, `match`, and destructuring
assignment.

```yed
let { index, value } = users.enumerate().at(0)
let { index: i, value: user } = users.enumerate().at(0)
let { value, .. } = users.enumerate().at(0)
```

Optional extraction composes with ordinary option patterns:

```yed
if let Some({ index, value: user }) = users.enumerate().first() {
    user.id
}
```

Closure parameters may destructure records directly:

```yed
users.enumerate().map(|{ index, value: user }| {
    index,
    id: user.id,
})
```

Destructuring assignment should reassign existing mutable places, not introduce
new bindings. An `=` token does not imply declaration; `let` is the declaration
syntax, and a bare pattern on the left-hand side is assignment syntax:

```yed
let mut i = 0
let mut user = default_user()

{ index: i, value: user } = users.enumerate().at(0)
```

The shorthand assignment form is only valid when the shorthand names already
refer to mutable locals:

```yed
let mut index = 0usize
let mut value = default_user()

{ index, value } = users.enumerate().at(0)
```

Use `let` when the destructuring operation introduces new locals:

```yed
let { index, value } = users.enumerate().at(0)
```

In expression position, `{ index, value }` remains a record literal. In pattern
position, it is a record pattern. The parser and HIR should keep this distinction
explicit so projection records, callback parameters, and assignment targets do
not require special query syntax.

This gives a simple, Rust-like rule:

- `let pattern = expr` binds new locals;
- `pattern = expr` writes into existing assignable places;
- `pattern <- expr` or other assignment-only punctuation is not needed.

This also means APIs should prefer named structural results when the result has
domain meaning:

```yed
enumerate() -> { index: usize, value: T }
rank_by(...) -> { value: T, index: usize, rank: usize, dense_rank: usize }
partition(...) -> { matched: [T], rest: [T] }
```

Tuples remain fine for small positional products with no semantic field names,
but query-native carriers should usually be records.

## Grouping

### Pipeline grouping

Pipeline grouping remains the best surface when grouping the main query stream:

```yed
select groups@g[*].{
    city: g.city,
    users: g.users@u[*].id,
}
from users@u:User
group by { city: u.city } into groups
```

Group objects should expose:

- direct key fields when unambiguous, e.g. `g.city`;
- `g.key()` as the collision-free canonical key accessor;
- `g.items()` as the collision-free canonical member collection accessor;
- for pipeline groups, the grouped root label as an ergonomic member field,
  e.g. `g.users`.

`g.values()` should not be the primary API. In a general-purpose language,
`values` is already associated with map/document values and is less precise
than `items` or `members`. Use `g.items()` as the canonical accessor.

If a key field collides with a group method or member label, use `g.key().field`.

### Expression grouping

Grouping should also be available as a normal method:

```yed
users@u[*]
    .group_by(|u| { city: u.city })
    .map(|g| {
        city: g.city,
        users: g.items()@u[*].id,
    })
```

The method form returns a collection of group objects with the same key/item
contract. Because no pipeline root label is available, `g.items()` is the
primary member accessor.

### Path grouping

A bracket group selector is acceptable because it transforms the current
collection value:

```yed
users@u[group by { city: u.city }]@g[*].{
    city: g.city,
    users: g.items()@u[*].id,
}
```

This is sugar for `.group_by(...)`. It should not replace pipeline grouping;
it is for expression-level collection shaping.

## Ordering

Path/pipeline order syntax remains:

```yed
users@u[order by u.age desc, u.id asc]
```

The canonical method form should use ordinary functions or typed wrappers:

```yed
users@u[*].order_by(|u| (order::desc(u.age), order::asc(u.id)))
```

Conveniences are fine:

```yed
users@u[*].order_by_desc(|u| u.age)
users@u[*].order_by_asc(|u| u.name)
```

`order::asc` and `order::desc` should be ordinary prelude-visible functions or
associated constructors. Fully qualified names remain available; bare `asc` and
`desc` are just normal imports/prelude bindings and can be shadowed. Native
recognition still depends on resolved identity.

Do not make `value.desc()` / `value.asc()` the canonical surface. That would put
short ordering words into every value's method namespace, where `desc` can also
mean "description" and where user methods should remain free to exist. If Yed
later wants method sugar, it should be an ordinary resolved extension trait over
orderable keys, and QIR should recognize it only by lang-item identity. The
production surface does not require that sugar because these forms are already
explicit and native:

```yed
users.order_by(|u| order::desc(u.score))
users.order_by_desc(|u| u.score)
users@u[order by u.score desc]
```

Use `order_by` for non-mutating collection/query values. Reserve `sort` or
`sort_by` for mutable/in-place collection APIs if Yed later adds them.

## Slicing, indexing, and range

Bracket ranges are collection slices:

```yed
users[0]
users[0..3]
users[0..=2]
users@u[order by u.name][0..10]
```

Method equivalents:

```yed
users.at(0)
users.get(0)
users.slice(0..3)
users.range(0..3)
users.skip(20).take(10)
users.take(10)
users.first()
users.last()
users.exactly_one()
users.expect_one()
```

Use `.slice(range)` / `.range(range)` for range-shaped arguments. `.skip(n)` and
`.take(n)` are conveniences.

Access contract:

- `[n]` and `.at(n)` return `T` and are runtime errors when the element is
  missing.
- `.get(n)`, `.first()`, and `.last()` return `Option<T>`.
- `.exactly_one()` returns `Result<T, CardinalityError>` and checks exact
  cardinality without panicking.
- `.expect_one()` returns `T` and asserts exactly one element. The name is
  intentionally assertion-shaped, like `expect` or `unwrap`: zero or many
  elements are observable runtime errors.

QIR may satisfy these contracts with static cardinality facts. Otherwise it must
preserve the runtime error/option behavior.

Query expressions are collection-valued by default, even when a programmer
expects zero or one result. Scalarization is explicit and should read like the
chosen contract:

```yed
users.first()           // maybe first by order
users.get(0)            // maybe element at index
users[0]                // error if missing, extra elements allowed
users.exactly_one()     // Result<T, CardinalityError>
users.expect_one()      // exactly one, error on zero or many
```

Pipeline range should use the same range expression model:

```yed
select users@u[*].{ id: u.id, name: u.name }
from users@u:User
order by u.name asc
range 20..30
```

`range 20..30` means the half-open interval `[20, 30)`, matching ordinary
collection slicing. It is equivalent to offset 20 and limit 10. Other useful
forms:

```yed
range ..10    // first ten
range 20..    // skip twenty
range 0..=9   // inclusive end, also first ten
```

`range` replaces separate `start` and `limit` pipeline keywords. Separate
clauses split one semantic operation into two syntax nodes, make dynamic bounds
harder to represent, and are less consistent with path selectors. Internally
QIR can still normalize a constant range into `offset + limit`.

Important: this means the following is a slice, not graph depth:

```yed
users@u[*].friends@f[1..3]
```

Recursive or variable-depth graph traversal needs graph-specific syntax or
library APIs, not bracket slicing.

## Mapping, filtering, and flattening

Existing path selectors:

```yed
users@u[*].{ id: u.id }
users@u[where u.active]
users@u[*].posts@p[**]
```

Collection-shaping selectors imply iteration for a following projection. An
extra `[*]` is accepted as an identity/all selector, but it is not required:

```yed
users@u[order by u.id].id
users@u[order by u.id].{ id: u.id, name: u.name }
```

Canonical methods:

```yed
users.map(|u| { id: u.id })
users.filter(|u| u.active)
users.flat_map(|u| u.posts)
users.flatten()
users.flatten(2) // compile-time depth only, if static-depth calls are enabled
```

`flatten()` is a one-level structural flatten. For deeper static flattening,
prefer repeated `.flatten()` calls or path selector stars such as `[**]` and
`[***]`. A runtime `.flat(depth: int)` is not part of the native surface:
flatten depth changes the static result type. Static depth is valid if the
argument is compile-time known, so a future `.flatten(2)` / const-depth form may
be native. It must not accept an ordinary runtime integer.

`filter_map` should be method/function only. There is no bracket form that is
clear enough to justify adding one:

```yed
users.filter_map(|u| {
    if u.active {
        Some({ id: u.id })
    } else {
        None
    }
})
```

QIR may lower native `filter_map` to `Filter + Project` when the closure is
effect-free and the option shape is recognized. If the closure can error, has
effects, or uses unrecognized user code, it stays a scalar/runtime operation or
a legal dependent fallback.

## Enumeration and position

Use `enumerate`, because it is familiar from general-purpose languages.

Yed should return a structural record rather than a tuple as the primary shape:

```yed
users@u[*]
    .enumerate()
    .map(|entry| {
        index: entry.index,
        user_id: entry.value.id,
    })
```

`entry.index` is zero-based, matching collection indexing. `entry.value` is the
original element.

An ergonomic helper can exist:

```yed
users@u[*].map_indexed(|u, index| {
    index,
    user_id: u.id,
})
```

Path sugar is optional and should be simple if added:

```yed
users[enumerate]@e[*].{
    index: e.index,
    user_id: e.value.id,
}
```

With record destructuring, the natural method form is:

```yed
users.enumerate().map(|{ index, value: user }| {
    index,
    user_id: user.id,
})
```

Avoid forms like `[* with index i]`; they make the selector grammar carry too
many local binding modes.

Do not add `.enumerate(|...|)`. That would mix a product-producing method with a
callback method and recreate the `|index, item|` versus `|item, index|`
ambiguity.

## Ranking

Ranking is not the same as enumeration. Enumeration gives positional indexes.
Ranking also has tie semantics.

Canonical method:

```yed
users@u[*]
    .rank_by(|u| order::desc(u.score))
    .map(|r| {
        user_id: r.value.id,
        index: r.index,
        rank: r.rank,
        dense_rank: r.dense_rank,
    })
```

`rank_by` returns a collection of ranked records:

```text
{
  value: T,
  index: usize,       // zero-based ordered position
  rank: usize,        // one-based tie-aware rank with gaps
  dense_rank: usize,  // one-based tie-aware rank without gaps
}
```

Grouped/partitioned ranking should be expressed by explicit grouping:

```yed
posts@p[*]
    .group_by(|p| { author_id: p.author_id })
    .map(|g| {
        author_id: g.author_id,
        posts: g.items()
            .rank_by(|p| order::desc(p.created_at))
            .map(|r| {
                id: r.value.id,
                rank_for_author: r.rank,
            }),
    })
```

QIR may fuse `group_by(...).rank_by(...)` into a native `Window` plan when
the closures are recognized and safe.

Do not add SQL-like `over` or `partition by` source syntax. The partition is
the explicit group.

## Aggregates and existence

Support both function and method forms where useful:

```yed
len(users)
count(users)
users.count()
sum(users@u[*].age)
users@u[*].age.sum()
min(xs)
max(xs)
avg(xs)
```

Existence and predicate methods:

```yed
users.is_empty()
users.any(|u| u.active)
users.all(|u| u.age >= 18)
users.none(|u| u.blocked)
```

These are native query intents when they resolve to standard-library identities.
They can lower to aggregate, semi/anti join, mark join, or scalar aggregate
decorrelation depending on context.

## Distinct and set operations

Canonical methods:

```yed
users.distinct()
users.distinct_by(|u| u.id)
users.union(other_users)
users.intersect(active_users)
users.except(blocked_users)
```

Path sugar is natural for distinct because it transforms the current collection:

```yed
users@u[distinct]
users@u[distinct by u.id]
```

Set operations should stay method/function based because they require another
collection operand.

## Joins

`links` remains the primary graph/storage relationship surface.

For non-graph collection joins, a method API can exist:

```yed
users.join_by(posts, |u, p| u.id == p.author_id)
users.left_join_by(posts, |u, p| u.id == p.author_id)
```

This should be a normal collection/query method, not a pipeline default. The
current multi-root `from users, posts` rule should remain independent roots,
not an implicit cross product or join.

Useful relation APIs:

```yed
users.join_by(posts, |u, p| u.id == p.author_id)
users.left_join_by(posts, |u, p| u.id == p.author_id)
users.semi_join_by(posts, |u, p| u.id == p.author_id)
users.anti_join_by(posts, |u, p| u.id == p.author_id)
users.zip(other)
```

`zip` is positional and therefore order-sensitive; it is a collection API, not a
relational join.

## Traversal and recursion

Keep traversal in `links` and path materialization. Do not add a vague `.walk`
core API.

One-hop and fixed-chain traversal:

```yed
from users@u:User
links (users)->[follows@e:Follows]->(friends@f:User)
```

Variable-depth bounded traversal should be graph-specific and explicit:

```yed
from users@u:User
links (users)->[follows@e:Follows hops 1..=3]->(friends@f:User)
```

The hop range belongs to the edge segment because it describes repeated
traversal of that relationship. Keep it local to `links`; do not reuse
collection slices.

Path access after `links` consumes bindings that the `links` clause introduced.
It is not implicit graph navigation through arbitrary fields:

```yed
select users@u[*].follows@e[**].friends@f[**].id
from users@u:User
links (users)->[follows@e:Follows hops 1..=3]->(friends@f:User)
```

The double star on the edge binding matters in a top-level projection. With
`users@u[*].follows@e[*]`, each selected user still carries a nested collection
of edge paths, so projecting through `friends` at top level would preserve an
outer nested collection and should be rejected unless the program explicitly
wants that nesting. For a flat top-level stream of reached friend ids, flatten
both the edge-path collection and the reached-node collection:

```yed
users@u[*].follows@e[**].friends@f[**].id
```

Inside a per-user record projection, `u.follows@e[*].friends@f[**].id` can be
valid because the nesting is intentionally scoped under that one `u`.

Without a `links` clause, `u.friends` is just an ordinary record/path field and
is valid only when the `User` value actually has such a field. A future method
surface may be added for graph traversal, but it must be explicit and resolved,
for example:

```yed
graph::traverse({
    from: users@u[*],
    edge: Follows,
    to: User,
    hops: 1..=3,
})
```

or a typed method equivalent backed by schema/index metadata. It must not be
magic property access.

For general recursive dataflow, use an explicit library function with precise
semantics:

```yed
graph::transitive_closure({
    seed: users@u[where u.id == start_id][*],
    step: |u| u.friends@f[*],
    key: |u| u.id,
})
```

`transitive_closure` is clearer than `reach`: it says this computes a recursive
closure and requires a key for duplicate elimination/termination.

QIR lowering:

- bounded `links ... hops ...` can lower to `RecursiveExpand`;
- unbounded or until-stable recursive closure lowers to `Fixpoint`;
- traversal-sensitive or effectful cases remain explicit barriers/fallbacks.

## Native standard-library surface

The source surface should expose these normal methods/functions. QIR only
recognizes them as native when resolution proves they are the standard-library
items and the required facts make the rewrite legal.

### Transform and projection

```yed
items.map(|item| expr)
items.filter(|item| pred)
items.filter_map(|item| if pred { Some(expr) } else { None })
items.flat_map(|item| many_items)
items.flatten()
items.compact()              // [Option<T>] -> [T]
```

`compact()` is useful when the option production already happened earlier.
`filter_map` combines option production and filtering. Both are native only when
the option identity is known.

### Bounds and cardinality

```yed
items.range(20..30)
items.slice(20..30)
items.skip(20)
items.take(10)
items.at(0)
items.get(0)
items.first()
items.last()
items.exactly_one()
items.expect_one()
items.is_empty()
items.len()
items.count()
```

`len` and `count` should be aliases only if the type system can keep their
domains clean. Prefer `len` for in-memory collections and `count` as the query
aggregate spelling; both may resolve to the same native aggregate when the
receiver is a query collection.

### Ordering and position

```yed
items.order_by(|item| order::asc(item.name))
items.order_by(|item| (order::desc(item.score), order::asc(item.id)))
items.order_by_asc(|item| item.name)
items.order_by_desc(|item| item.score)
items.reversed()
items.enumerate()
items.map_indexed(|item, index| expr)
items.rank_by(|item| order::desc(item.score))
```

`reversed()` is native only when the input has a known order. Otherwise it is a
materialized collection operation. Reserve `reverse` / `sort` names for
mutating in-place APIs if Yed adds them.

### Grouping, indexing, and lookup

```yed
items.group_by(|item| key)
items.index_by(|item| key)
items.key_by(|item| key)
items.associate_by(|item| key, |item| value)
```

`group_by` returns group records. `index_by` returns a query-native
`Lookup<K, [T]>` from key to all matching items. `key_by` asserts key uniqueness
and returns `Lookup<K, T>`. `associate_by` maps key to a projected value and
returns `Lookup<K, V>`. QIR may use uniqueness facts from `key_by` for
decorrelation and join planning; if uniqueness is not proven, it must preserve
the runtime uniqueness error.

`Lookup` is the native query lookup carrier, not the whole general-purpose map
library. It is intentionally small because it carries optimizer facts:
cardinality, uniqueness, deterministic lookup shape, and key/value scalar
kernels.

General collection families should exist in the normal standard library, but
they are distinct from the query-native carrier:

| Family | Purpose | Query/native relationship |
| --- | --- | --- |
| `[T]` / `Array<T>` | ordered contiguous collection, default query result | primary collection carrier |
| `Lookup<K, V>` | optimizer-visible query lookup/multimap result | native QIR `LookupBuild` |
| `HashSet<T>` | unordered unique membership | native only for proven set/membership rewrites |
| `BTreeSet<T>` | ordered unique membership/range queries | native only when order/range facts are needed |
| `HashMap<K, V>` | unordered key/value store | ordinary stdlib map unless lowered to lookup facts |
| `BTreeMap<K, V>` | ordered key/value store and range iteration | ordinary stdlib map unless order/range facts are preserved |
| `Deque<T>` | queue/stack-style front/back operations | runtime collection unless rewritten to bounded stream ops |
| `BinaryHeap<T>` | priority queue / top-k user structure | runtime collection; QIR has separate `TopN`/top-k carriers |

Conversions should be ordinary methods once these types exist:

```yed
items.to_array()
items.to_hash_set()
items.to_btree_set()
items.to_hash_map(|item| key, |item| value)
items.to_btree_map(|item| key, |item| value)
map.entries()
map.keys()
map.values()
set.contains(value)
btree.range(10..=20)
```

QIR should recognize those conversions only when the resolved identities and
facts make the conversion semantically visible and legal to optimize. Otherwise
they remain normal VM/std-library collection operations.

### Aggregates and reductions

```yed
items.sum()
items.min()
items.max()
items.avg()
items.fold(init, |acc, item| next)
items.reduce(|acc, item| next)
items.scan(init, |state, item| next_state)
```

`sum/min/max/avg/count` are native aggregate identities. `fold/reduce/scan` are
general-purpose collection APIs; they are not automatically relational
aggregates unless the resolved function is a recognized algebraic reducer with
safe associativity/identity/error facts.

### Existence and predicates

```yed
items.any(|item| pred)
items.all(|item| pred)
items.none(|item| pred)
items.contains(value)
items.contains_by(|item| key, value)
```

Predicate methods are decorrelation candidates in value or predicate position.
`all(pred)` lowers through anti-join style logic over `!pred` when safe.

### Split, distinct, and set operations

```yed
items.partition(|item| pred)       // { matched: [T], rest: [T] }
items.distinct()
items.distinct_by(|item| key)
items.union(other)
items.intersect(other)
items.except(other)
```

`partition` is two filters with shared input when the predicate is safe. Set
operations preserve the chosen set/bag semantics of the concrete API; do not
hide bag-vs-set behavior behind one name.

### Relation and graph operations

```yed
items.join_by(other, |left, right| pred)
items.left_join_by(other, |left, right| pred)
items.semi_join_by(other, |left, right| pred)
items.anti_join_by(other, |left, right| pred)
items.zip(other)
graph::transitive_closure({ seed, step, key })
```

Graph edge traversal remains the `links` surface. Collection relation methods
are ordinary APIs over values and lower to joins only when the facts support it.

Domain-specific native surfaces such as geo indexes, full-text/vector search,
tree traversal, graph algorithms, numeric/statistical kernels, and binary-search
helpers should follow the same rule: first define an ordinary typed stdlib API,
then attach QIR native meaning only by resolved identity and only when the
operation has precise optimizer facts. They should not be smuggled into the core
query grammar as new global keywords.

See `standard-library-data-structures-and-algorithms.md` for the broader
standard-library surface and the query-native eligibility matrix.

## Path and pipeline sugar inventory

| Operation | Canonical method/function | Path/pipeline surface | QIR/native target |
| --- | --- | --- | --- |
| Map | `.map(|x| y)` | `@x[*].expr` | `Project`, `Compute` |
| Filter | `.filter(|x| pred)` | `[where pred]`, pipeline `where` | `Filter`, predicate pushdown |
| Filter-map | `.filter_map(|x| Option<Y>)` | none initially | `Filter + Project` when safe |
| Compact | `.compact()` | none initially | filter known `Some` payloads |
| Flat-map | `.flat_map(...)`, `.flatten()` | `[**]`, `[***]` | flatten/leaf stream hint, `Nest`/unnest |
| Order | `.order_by(...)` | `[order by ...]`, pipeline `order by` | `Order`, `Sort`, `TopN` |
| Slice/range | `.slice(r)`, `.range(r)`, `.skip(n).take(m)` | `[a..b]`, `[a..=b]`, pipeline `range` | `LimitOffset`, per-left range |
| Erroring index/cardinality | `.at(n)`, `.expect_one()` | `[n]` | scalar selector, `TopKPerKey` if correlated |
| Optional access | `.get(n)`, `.first()`, `.last()` | none initially | `Option<T>` scalar extraction |
| Checked cardinality | `.exactly_one()` | none initially | `Result<T, CardinalityError>`; native only when result construction is preserved |
| Group | `.group_by(...)` | pipeline `group by`, optional `[group by]` | `Aggregate`, `GroupJoin`, group materialization |
| Lookup maps | `.index_by/.key_by/.associate_by` | none initially | `LookupBuild`, uniqueness contracts, lookup decorrelation |
| General maps/sets | `.to_hash_map`, `.to_btree_map`, `.to_hash_set`, `.to_btree_set` once stdlib types exist | none initially | ordinary runtime unless resolved facts justify lookup/set/order carriers |
| Aggregate | `len/count/sum/min/max/avg` | none beyond ordinary calls | `Aggregate`, `GroupJoin` |
| General reduction | `.fold/.reduce/.scan` | none | scalar/runtime unless recognized algebraic reducer |
| Binary search | `.binary_search/.lower_bound/.upper_bound` | none | range/index lookup only with known ordering facts |
| Existence | `.any/.all/.none/.is_empty` | ordinary calls over selectors | `Semi`, `Anti`, `Mark` |
| Enumerate | `.enumerate()`, `.map_indexed(...)` | optional `[enumerate]` | `Window` row-number-like carrier |
| Rank | `.rank_by(...)` | none initially | `Window` rank/dense-rank carrier |
| Partition | `.partition(...)` | none initially | shared input plus two filters when safe |
| Distinct | `.distinct()`, `.distinct_by(...)` | `[distinct]`, `[distinct by ...]` | `SetOp`, aggregate/dedup |
| Set ops | `.union/.intersect/.except` | none | `SetOp` |
| Join | `.join_by/.left_join_by/.semi_join_by/.anti_join_by` | none; `links` for graph relationships | `Join`, `Semi`, `Anti` |
| Zip | `.zip(...)` | none | order-sensitive positional operator |
| Links traversal | `links` syntax; path selectors consume result | `links (...) -> [...] -> (...)` | `Traverse`, `RecursiveExpand` |
| Recursive closure | `graph::transitive_closure(...)` | graph-specific `hops` in `links` for bounded cases | `Fixpoint` |
| Decimal/money | `decimal`, `money`, `.round/.add/.convert` | none | scalar kernels/aggregates only with fixed rounding/currency semantics |
| Geo/search | `geo::*`, `search::*`, `index.search(...)` | none initially | provider-backed index scan, KNN, spatial join, full-text/vector search |

## Optimizer contract

Every native method/function/path selector must lower through the same facts:

```text
source Yed
  -> HIR
  -> typecheck/resolution/lang-item identity
  -> THIR expression + metadata
  -> QIR value facts and native intent facts
  -> logical QIR
  -> decorrelation / aggregate pushdown / join reorder / physical planning
  -> exec DAG + scalar bytecode kernels
```

Optimizations are legal only when QIR proves the relevant facts:

- deterministic;
- effect-free;
- runtime-error movement is not observable;
- materialization order is not observable, or the materialization is preserved;
- closures and order/group/aggregate keys are compiled as scalar kernels;
- outer references are represented in `ValueExprId` facts for decorrelation;
- cardinality contracts such as `.expect_one()` and `key_by` uniqueness are either
  statically proven or preserved as runtime checks;
- distribution/order/cardinality facts are preserved across rewrites.

If those facts do not hold, QIR must leave an explicit dependent `Apply`,
barrier, or scalar runtime operation with verifier-visible reasons. The source
surface should stay expressive; the backend decides which expression islands are
native relational algebra.

## Design decisions

- Keep pipeline clauses. They define storage-stream placement and are not
  pointless.
- Use pipeline `range` as the canonical bounded-stream clause. Separate
  `start` / `limit` are not part of the final query-tail syntax; `range ..n`
  and `range a..b` cover both cases with the same model as selectors.
- Keep methods/functions canonical. They make the language feel like a
  general-purpose language.
- Add path selector sugar only when it preserves the obvious `[...]` collection
  transform model.
- Use `g.items()` as the canonical group-member accessor, not `g.values()`.
- Allow direct key fields like `g.city` when unambiguous; keep `g.key()` as the
  collision-free fallback.
- Use `enumerate()` for zero-based index/value records; use
  `map_indexed(|item, index| ...)` when a callback helper is desired; optionally
  add `[enumerate]` later.
- Use `.at(n)` for erroring positional extraction and `.get(n)` / `.first()` /
  `.last()` for optional access. Use `.exactly_one()` when the program should
  handle cardinality as data and `.expect_one()` only when the program is making
  an explicit assertion.
- Treat query and subquery expressions as collection-valued unless an explicit
  scalarization method or selector states otherwise.
- Use `!` as the eventual source-level bottom type spelling. The `Never` lang
  item may remain as a named stdlib carrier while the parser/type surface is
  migrated.
- Support record destructuring in closure parameters, lets, matches, loops, and
  destructuring assignment. Native product results should prefer records over
  tuples when field names carry meaning.
- Keep `fold`, `reduce`, and `scan` as general-purpose collection APIs; treat
  them as native relational aggregates only for resolved, proven algebraic
  reducers.
- Use `rank_by(...)` for tie-aware ranking; lower to QIR `Window` when native.
- Use `order::asc`/`order::desc` wrappers for method-based ordering; path
  `order by ... asc/desc` remains ergonomic selector/pipeline syntax. Keep
  `order_by_desc` / `order_by_asc` as collection-level conveniences; do not make
  global `value.desc()` / `value.asc()` canonical.
- Do not use `.walk` as a core traversal concept. Keep traversal in `links` and
  use explicit `hops`/`transitive_closure` for recursive graph work.
- Do not infer graph edges from field names. Direct `u.friends` is only direct
  field/path access; edge relationships need `links` or an explicit graph API.
- Do not add named arguments as a special call form for native query APIs. Use
  ordinary positional arguments for small APIs and config objects for APIs with
  several named concepts.
- Keep `[T]` as the default query/subquery result collection. `Lookup<K, V>` is
  the native query lookup carrier; full `HashMap`/`BTreeMap`/`HashSet`/
  `BTreeSet`/queue/heap families belong to the general standard library and
  become native only through resolved identities plus optimizer facts.
