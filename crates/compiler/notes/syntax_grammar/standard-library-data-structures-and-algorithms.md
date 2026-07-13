# Standard Library Data Structures, Algorithms, and Query-Native Eligibility

Status: source/API contract for the standard library surface.

This note defines the broader Yed standard-library direction for data
structures and algorithms that are useful both in ordinary programs and in query
contexts. These APIs should be ordinary values, methods, functions, modules,
traits, and types. They should not become new global query keywords. QIR may
lower selected APIs to native logical/physical operators only when resolution
proves the standard-library identity and optimizer facts make the rewrite legal.

Implementation status:

- `std::query` array/lookup/native-collection methods are executable
  lang-item-backed operations today and are the primary QIR/VM-native surface.
- `std::collections`, `std::algo`, `std::math`, `std::stats`, `std::time`,
  `std::decimal`, `std::money`, `std::geo`, `std::search`, `std::text`,
  `std::tree`, and `std::bytes` are now wired into the default stdlib as
  ordinary modules. Some small pure helpers are executable through existing
  builtins or collection methods; domain operations such as decimal arithmetic,
  money conversion, geo/search/tree/time-provider work remain API contracts
  until a VM lang item, runtime provider, or backend capability is deliberately
  added.
- Do not treat a stdlib API as query-native because of its spelling. Native
  lowering requires resolved identity plus the legality checks below.
- Implementation work for making more of this surface query-native is tracked
  in
  `crates/lang/notes/docs/refactor/query-native-stdlib-eligibility-checklist.md`.

## Native Eligibility Rule

An operation is query-native only when all of these are true:

- the callee resolves to a standard-library identity or lang item;
- the operation is deterministic and effect-free for the values being moved;
- runtime errors, panics, and option/result/cardinality behavior are preserved;
- ordering, grouping, uniqueness, distribution, and cardinality facts are either
  preserved or represented by explicit runtime checks;
- scalar keys/predicates/projections are compiled as scalar kernels;
- the physical backend has the required capability, index, or fallback plan;
- distributed execution can preserve the same partitioning/order semantics.

If these facts are missing, the operation remains an ordinary runtime/VM
operation. This keeps Yed expressive without making the optimizer unsound.

## Module Shape

Use normal modules:

```yed
std::collections
std::algo
std::math
std::stats
std::geo
std::search
std::graph
std::tree
std::time
std::bytes
std::text
```

Prelude exports should stay small: core collection methods, `Option`, `Result`,
common numeric functions, and query carriers that are already language-level.
Specialized domains such as geo/search/graph should usually be imported.

## Core Collection Families

### Array

`[T]` / `Array<T>` is the default query/subquery result collection and the
primary ordered collection carrier.

Core methods:

```yed
xs.len()
xs.is_empty()
xs.at(index)
xs.get(index)
xs.first()
xs.last()
xs.slice(range)
xs.range(range)
xs.skip(n)
xs.take(n)
xs.map(|x| y)
xs.filter(|x| pred)
xs.filter_map(|x| maybe_y)
xs.flat_map(|x| ys)
xs.flatten()
xs.compact()
xs.fold(init, |acc, x| next)
xs.reduce(|acc, x| next)
xs.scan(init, |state, x| { left: next_state, right: y })
xs.any(|x| pred)
xs.all(|x| pred)
xs.none(|x| pred)
xs.contains(value)
xs.contains_by(|x| pred)
xs.order_by(|x| order::desc(x.score))
xs.order_by_asc(|x| key)
xs.order_by_desc(|x| key)
xs.reversed()
xs.enumerate()
xs.map_indexed(|x, index| y)
xs.rank_by(|x| key)
xs.group_by(|x| key)
xs.index_by(|x| key)
xs.key_by(|x| key)
xs.associate_by(|x| key, |x| value)
xs.partition(|x| pred)
xs.distinct()
xs.distinct_by(|x| key)
xs.union(ys)
xs.intersect(ys)
xs.except(ys)
xs.join_by(ys, |x, y| pred)
xs.left_join_by(ys, |x, y| pred)
xs.semi_join_by(ys, |x, y| pred)
xs.anti_join_by(ys, |x, y| pred)
xs.zip(ys)
```

Query-native targets: `Project`, `Filter`, `LimitOffset`, `TopN`, `Window`,
`Aggregate`, `LookupBuild`, `SetOp`, `Join`, `Semi`, `Anti`, `Mark`,
`RecursiveExpand` when the operation comes from graph/path lowering.

### Lookup

`Lookup<K, V>` is the query-native lookup/multimap carrier produced by
`index_by`, `key_by`, and `associate_by`. It is intentionally smaller than a
general map because it carries optimizer contracts.

Core methods:

```yed
lookup.len()
lookup.is_empty()
lookup.entries()
lookup.keys()
lookup.values()
lookup.get(key)
lookup.get_many(key)
lookup.contains_key(key)
lookup.map_values(|value| next)
lookup.into_array()
```

Query-native targets: `LookupBuild`, uniqueness/cardinality checks, hash or
range lookup, decorrelated correlated-subquery lookup.

### Sets

`HashSet<T>` is for unordered membership; `BTreeSet<T>` is for ordered
membership and range iteration.

Core methods:

```yed
set.len()
set.is_empty()
set.contains(value)
set.with(value)          // persistent/functional API
set.without(value)       // persistent/functional API
set.union(other)
set.intersect(other)
set.except(other)
set.symmetric_difference(other)
set.is_subset(other)
set.is_superset(other)
set.to_array()

btree.first()
btree.last()
btree.range(start..end)
btree.lower_bound(key)
btree.upper_bound(key)
```

Query-native targets: deduplication, set membership semi/anti joins, set
operations, range-index scans for ordered sets when backed by an index.

### Maps

`HashMap<K, V>` is for unordered key/value storage; `BTreeMap<K, V>` is for
ordered key/value storage and range iteration.

Core methods:

```yed
map.len()
map.is_empty()
map.contains_key(key)
map.get(key)
map.get_or(key, fallback)
map.with(key, value)         // persistent/functional API
map.without(key)             // persistent/functional API
map.keys()
map.values()
map.entries()
map.map_values(|value| next)
map.filter_entries(|key, value| pred)
map.merge(other, |key, left, right| value)
map.to_array()

btree.range(start..end)
btree.lower_bound(key)
btree.upper_bound(key)
```

Query-native targets: lookup build/probe, functional dependency propagation,
point/range lookup, join planning, index scan pushdown.

### Deques, Queues, and Heaps

Use `Deque<T>` for front/back queue work and `BinaryHeap<T>` /
`PriorityQueue<T, P>` for priority access.

Core methods:

```yed
deque.len()
deque.is_empty()
deque.push_front(value)
deque.push_back(value)
deque.pop_front()
deque.pop_back()
deque.front()
deque.back()
deque.to_array()

heap.len()
heap.is_empty()
heap.push(value)
heap.peek()
heap.pop()
heap.into_sorted_array()
heap.top_k(k)
```

Query-native targets: queues are usually runtime structures; priority queues may
lower to `TopN`/top-k only when the operation is pure and equivalent to an
ordered limit.

## Algorithms

### Search and Binary Search

These are general algorithms over sorted collections or indexed data.

```yed
algo::binary_search(xs, key)
algo::binary_search_by(xs, |x| compare(x, key))
algo::lower_bound(xs, key)
algo::upper_bound(xs, key)
algo::equal_range(xs, key)
algo::partition_point(xs, |x| pred)
algo::top_k(xs, k, |x| score)

xs.binary_search(key)
xs.binary_search_by(key, |x, key| compare(x, key))
xs.lower_bound(key)
xs.upper_bound(key)
xs.equal_range(key)
xs.partition_point(|x| pred)
xs.top_k(k, |x| score)
```

Query-native targets: range scans, ordered index lookup, `TopN`, per-left TopK.
The method form is canonical when the sorted collection is the receiver; the
module form is useful when passing algorithms around or avoiding method
namespace conflicts. Binary-search APIs are native only when QIR knows the input
ordering and comparison/key semantics.

### Text, Full-Text, and Vector Search

Keep text and search APIs in `std::text` and `std::search`.

```yed
text::normalize(s)
text::tokens(s)
text::ngrams(s, n)
text::contains(haystack, needle)
text::starts_with(s, prefix)
text::ends_with(s, suffix)
text::regex_match(pattern, s)

search::text(index, query)
search::full_text(index, { text, limit })
index.search(search::full_text_query(text, limit))
search::phrase(index, { phrase, slop, limit })
search::prefix(index, prefix)
search::regex(index, pattern)
search::vector(index, { embedding, k: 10 })
search::hybrid(text_index, vector_index, { text: query, embedding, k: 10 })
```

Query-native targets: inverted-index scans, prefix/range scans, vector KNN,
hybrid search, scoring/order carriers. Regex stays runtime unless the storage
engine proves index support.

### Math

Math functions are ordinary numeric functions. Keep them small, typed, and
importable.

```yed
math::abs(x)
math::sign(x)
math::min(a, b)
math::max(a, b)
math::clamp(x, lo, hi)
math::pow(x, n)
math::sqrt(x)
math::log(x)
math::log2(x)
math::log10(x)
math::exp(x)
math::sin(x)
math::cos(x)
math::tan(x)
math::floor(x)
math::ceil(x)
math::round(x)
math::is_nan(x)
math::is_finite(x)

x.abs()
x.sign()
x.min(y)
x.max(y)
x.clamp(lo, hi)
x.pow(n)
x.sqrt()
```

Query-native targets: scalar kernels and constant folding. Only deterministic,
well-specified numeric behavior should be pushed across filters/joins.
Today the VM-native builtin subset is mostly i64 (`abs`, `pow`, i64
aggregates). Floating-point and decimal methods are standard-library contracts
until the VM/QIR scalar-kernel catalog has precise NaN, overflow, rounding, and
determinism rules.

### Statistics and Analytics

Use `std::stats` for aggregate and window-friendly analytics.

```yed
stats::count(xs)
stats::sum(xs)
stats::mean(xs)
stats::variance(xs)
stats::stddev(xs)
stats::min(xs)
stats::max(xs)
stats::median(xs)
stats::quantile(xs, q)
stats::histogram(xs, buckets)
stats::correlation(xs, ys)
stats::covariance(xs, ys)
stats::linear_regression(points)
stats::moving_average(xs, window)
stats::rank(xs, |x| key)

xs.sum()
xs.mean()
xs.min()
xs.max()
xs.variance()
xs.stddev()
xs.median()
xs.quantile(q)
```

Query-native targets: aggregate pushdown, partial/final aggregation,
distributed aggregation, windows, histograms, sketches where the API explicitly
chooses approximate behavior.

### Decimal, Money, and Precision

Financial and exact decimal data should not be represented with binary floats.
Use decimal and money values with explicit scale, currency, and rounding.

```yed
let amount = decimal(1299, 2)          // 12.99
let parsed = decimal_parse("12.99")
let rounded = amount.round(2, RoundingMode::HalfEven)

let usd = currency("USD")
let price = money(amount, usd)
price.amount()
price.currency()
price.round(2, RoundingMode::HalfEven)
price.add(other_price)

let rate = exchange_rate(currency("USD"), currency("CAD"), decimal_parse("1.37"))
price.convert(rate, 2, RoundingMode::HalfEven)
```

Query-native targets: exact decimal scalar kernels, aggregate pushdown over
decimals, currency-safe money aggregation, and checked conversion only after
the runtime/provider has fixed rounding, overflow, and currency mismatch
semantics. Money arithmetic should reject or return errors for mismatched
currencies; it must never silently add different currencies.

### Geo and Mapping

Use `std::geo` for geometry and spatial index operations. Do not add geo as
query keywords.

Core types:

```yed
geo::Point
geo::LineString
geo::Polygon
geo::MultiPoint
geo::MultiLineString
geo::MultiPolygon
geo::Rect
geo::BoundingBox
geo::Geohash
geo::H3Cell
```

Core methods/functions:

```yed
geo::point(lon, lat)
geo::bbox(min_lon, min_lat, max_lon, max_lat)
shape.bounding_box()
shape.centroid()
shape.area()
shape.length()
shape.distance(other)
shape.haversine_distance(other)
shape.contains(other)
shape.within(other)
shape.intersects(other)
shape.overlaps(other)
shape.touches(other)
shape.buffer(distance)
shape.simplify(tolerance)
shape.transform(crs)
geo::within_radius(index, point, radius)
geo::nearest(index, point, { k: 10 })
geo::covering_cells(shape, resolution)
geo::tile_bounds(z, x, y)
```

Query-native targets: bounding-box pushdown, spatial index scans, KNN spatial
search, radius search, partition pruning by geohash/H3, distributed spatial
joins. Exact geometry predicates must preserve the two-phase model: index
candidate generation followed by exact predicate verification.
Coordinates and distances are `f64` at the API boundary; exact geographic
semantics come from the chosen provider/CRS.

### Graph

Use `links` for schema-backed query traversal and `std::graph` for ordinary
graph algorithms.

```yed
graph::neighbors(graph, node)
graph::out_edges(graph, node)
graph::in_edges(graph, node)
graph::traverse({ from, edge, to, hops })
graph::transitive_closure({ seed, step, key })
graph::shortest_path({ graph, source, target, weight })
graph::connected_components(graph)
graph::strongly_connected_components(graph)
graph::topological_sort(graph)
graph::pagerank(graph, { damping, iterations })
```

Query-native targets: `Traverse`, `RecursiveExpand`, `Fixpoint`, semi/anti
reachability, graph index scans, distributed frontier expansion. Recursive
algorithms need explicit termination/deduplication keys.

### Trees

Use `std::tree` for hierarchical structures that are not necessarily graph
storage links.

```yed
tree::children(node)
tree::parent(node)
tree::ancestors(node)
tree::descendants(node)
tree::depth(node)
tree::path_to_root(node)
tree::lowest_common_ancestor(a, b)
tree::preorder(root)
tree::postorder(root)
tree::level_order(root)
```

Query-native targets: recursive expand/fixpoint when backed by stored parent or
child edges; otherwise runtime traversal.

### Time and Ranges

Time APIs are normal functions and types.

```yed
time::now()
time::date(year, month, day)
time::datetime("2026-06-08T00:00:00Z")
time::datetime_parse(s)
time::duration("PT1H")
time::duration_parse(s)
time::duration_seconds(60)
duration.total_seconds()
time::format(value, format)
time::truncate(value, unit)
time::bucket(value, duration)
time::timezone(value, zone)

range.contains(value)
range.overlaps(other)
range.intersect(other)
range.union(other)
range.is_empty()
```

Query-native targets: time-range scan pushdown, time bucketing, partition
pruning, range joins, range overlap predicates.

## Path Expression Relationship

Path expression sugar should exist only for collection/path shaping:

```yed
users@u[where u.active]
users@u[order by u.id]
users@u[1..=10]
users@u[distinct by u.email]
users@u[group by { city: u.city }]
users@u[*].links@e[**].targets@t[**]
```

Do not add bracket sugar for everything in this document. Maps, sets, heaps,
geo, search, math, stats, graph algorithms, and tree algorithms should first be
ordinary APIs. Path sugar is justified only when it remains an obvious selector
over the current collection.

## Distributed and Storage Contract

Native lowering must preserve backend capability boundaries:

- arrays/lists map to streams, materialized arrays, or query result batches;
- lookup/map/set carriers map to hash/range indexes only when keys are typed and
  equality/order semantics are known;
- ordered collections require order facts or explicit sorting;
- top-k/heap forms require deterministic tie behavior;
- geo/search require index capability plus exact predicate/scoring verification;
- recursive graph/tree operations require frontier state, dedup keys, and
  termination policy;
- statistical aggregates require decomposable partial/final forms before
  distributed pushdown;
- non-decomposable or approximate algorithms must say so in the API type/name.

This lets Yed remain a general-purpose language while still giving QIR enough
structure to optimize aggressively when the facts are real.
