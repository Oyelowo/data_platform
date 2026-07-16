# Storage Engines Master Plan

> **Scope:** This document defines the architecture, implementation order, and
correctness criteria for the data-platform storage layer. It sits in the
`storage_engines` crate because that crate is the root of the storage hierarchy.
Consensus, replication, and distributed coordination are explicitly out of scope
for the first phases; they will be added later as orthogonal layers on top of
stable engines.

---

## 1. Strategic decision: storage first, consensus later

**Yes, this is the correct order.**

Consensus (Raft, Paxos, chain replication) solves the problem of *agreeing on
a sequence of operations across machines*. A consensus layer needs a stable,
correct local storage engine underneath it for:

* its own persistent log,
* snapshots,
* state-machine application.

Trying to design both simultaneously creates a circular dependency: the
consensus layer needs a WAL, but the WAL needs crash-recovery semantics that are
only clear once the storage engine is well understood.

Therefore:

1. Build single-node storage engines that are **correct and durable**.
2. Add a local WAL abstraction that any engine can use.
3. Add replication/consensus as a separate crate that consumes the storage
   traits.

This is the same path taken by RocksDB→TiKV/Raft, LevelDB→CockroachDB, and
BerkeleyDB→FoundationDB.

---

## 2. What "complete" and "production-ready" mean here

It is **not possible** to build one engine that covers *every* conceivable data
platform use case with 100% standard compliance, because:

* "Data platform" spans OLTP, OLAP, HTAP, time-series, graph, vector search,
  object storage, streaming, and ledger systems.
* Standards in this space are fragmented: SQL is a family of standards, Parquet
  and Arrow have format specs, but most engine behavior is defined by
  implementation precedent.
* Correctness and performance are often in tension; a single engine cannot be
  optimal for all workloads.

What **is** achievable:

* A **family of storage engines** behind common traits.
* Each engine is **correct for its stated contract**, with a conformance test
  suite proving it.
* Each engine is **durable** under crashes, **thread-safe**, and observable.
* The suite as a whole covers the storage categories that a modern data platform
  needs.

We therefore target **100% contract compliance**, not mythical universal
completeness. Every engine documents its contract, its trade-offs, and the
standards it follows.

---

## 3. Core design principles

1. **Traits first, implementations second.**  
   All engines implement the same small set of traits. Callers depend on traits,
   not concrete engines.

2. **Layers, not monoliths.**  
   A high-level engine (e.g. document store) is built by composing lower-level
   engines (KV + columnar + index), not by reimplementing everything.

3. **Crash correctness is non-negotiable.**  
   Every durable engine must recover to a consistent state after a crash. This
   is proven by WAL + checkpoint design, not by testing alone.

4. **Concurrency is part of the contract.**  
   Isolation levels, lock granularity, and snapshot behavior are specified, not
   accidental.

5. **Test the contract, not the implementation.**  
   A shared conformance suite in `storage-traits/tests` must pass for every
   engine. New engines become valuable the moment they pass the suite.

6. **No `unwrap`/`expect` in library code.**  
   Every fallible operation returns a typed `Result`. This is already a project
   rule and applies here.

7. **Performance is measured, not assumed.**  
   Every engine ships with Criterion benchmarks and is validated against
   workload generators such as YCSB, TPC-C, TPC-H, and TSBS where applicable.

---

## 4. Architecture overview

```text
┌─────────────────────────────────────────────────────────────────────────────┐
│                          Query / Compute Layer                               │
│              (SQL parser, planner, optimizer, execution)                     │
└─────────────────────────────────────────────────────────────────────────────┘
                                       │
                                       ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                        Storage Federation Layer                              │
│   (route scans, joins, transactions across engines; caching; tiering)       │
└─────────────────────────────────────────────────────────────────────────────┘
                                       │
        ┌──────────────┬───────────────┼───────────────┬──────────────┐
        ▼              ▼               ▼               ▼              ▼
   ┌─────────┐   ┌──────────┐   ┌───────────┐   ┌──────────┐   ┌──────────┐
   │   KV    │   │ Columnar │   │  Document │   │  Graph   │   │  Vector  │
   │ engines │   │  engine  │   │   engine  │   │  engine  │   │  engine  │
   └────┬────┘   └────┬─────┘   └─────┬─────┘   └────┬─────┘   └────┬─────┘
        │             │               │              │              │
        └─────────────┴───────────────┴──────────────┴──────────────┘
                                       │
                                       ▼
                  ┌──────────────────────────────────────┐
                  │         Storage Traits               │
                  │  Engine, Transaction, Cursor, Blob   │
                  └──────────────────────────────────────┘
                                       │
        ┌──────────────────────────────┼──────────────────────────────┐
        ▼                              ▼                              ▼
   ┌─────────┐                   ┌──────────┐                   ┌──────────┐
   │   WAL   │                   │  Index   │                   │ Object   │
   │  crate  │                   │  crate   │                   │  store   │
   └─────────┘                   └──────────┘                   └──────────┘
```

### Crate graph

```text
storage-traits                 // public API: Engine, Transaction, Cursor, BlobStore, etc.
    │
    ├── storage-memory         // in-memory B-tree / skip-map for tests & caches
    ├── storage-wal            // append-only write-ahead log
    ├── storage-kv             // LSM-tree and B-tree key-value engines
    ├── storage-columnar       // Arrow in-memory + Parquet/ORC on disk
    ├── storage-document       // JSON/BSON document layer over KV/columnar
    ├── storage-graph          // graph store (adjacency + property indexes)
    ├── storage-time-series    // TSDB with compression and retention
    ├── storage-vector         // ANN indexes (HNSW, IVF)
    ├── storage-search         // inverted index / full-text search
    ├── storage-object         // large-object store with chunking & checksums
    ├── storage-cache          // tiered cache with eviction policies
    └── storage-federation     // router / planner across engines
```

No engine crate depends on another engine crate. They all depend on
`storage-traits` and on shared utility crates (`storage-wal`, `storage-index`,
etc.).

---

## 5. The trait layer

This is the most important crate. Get it wrong and every engine pays the price.

### 5.1 Base engine traits

```rust
use bytes::Bytes;
use std::ops::Range;
use std::sync::Arc;

/// A storage engine that can be opened, closed, and queried.
pub trait Engine: Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;
    type Transaction: Transaction<Error = Self::Error>;
    type Cursor: Cursor<Error = Self::Error>;

    /// Engine display name (for metrics and logging).
    fn name(&self) -> &'static str;

    /// Begin a new transaction.
    fn begin(&self, opts: TxnOptions) -> Result<Self::Transaction, Self::Error>;

    /// Single-key read outside a transaction (snapshot isolation).
    fn get(&self, key: &[u8]) -> Result<Option<Bytes>, Self::Error>;

    /// Ordered scan over a key range.
    fn scan(&self, range: Range<&[u8]>) -> Result<Self::Cursor, Self::Error>;

    /// Engine-wide statistics.
    fn stats(&self) -> Result<EngineStats, Self::Error>;

    /// Flush all dirty data to stable storage.
    fn sync(&self) -> Result<(), Self::Error>;
}

/// Transaction contract. Implementations choose the isolation mechanism.
pub trait Transaction: Sized + Send {
    type Error: std::error::Error + Send + Sync + 'static;

    fn get(&self, key: &[u8]) -> Result<Option<Bytes>, Self::Error>;
    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<(), Self::Error>;
    fn delete(&mut self, key: &[u8]) -> Result<(), Self::Error>;

    /// Ordered scan within the transaction's snapshot.
    fn scan(&self, range: Range<&[u8]>) -> Result<impl Cursor<Error = Self::Error>, Self::Error>;

    fn commit(self) -> Result<(), Self::Error>;
    fn rollback(self) -> Result<(), Self::Error>;
}

/// Ordered iterator over key-value pairs.
pub trait Cursor: Iterator<Item = Result<(Bytes, Bytes), Self::Error>> {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Move to the first key >= `target`.
    fn seek(&mut self, target: &[u8]) -> Result<(), Self::Error>;
}

#[derive(Clone, Debug, Default)]
pub struct TxnOptions {
    pub read_only: bool,
    pub isolation: IsolationLevel,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IsolationLevel {
    ReadUncommitted,
    #[default]
    ReadCommitted,
    RepeatableRead,
    Serializable,
    Snapshot,
}

#[derive(Clone, Debug, Default)]
pub struct EngineStats {
    pub name: &'static str,
    pub disk_bytes: u64,
    pub memory_bytes: u64,
    pub num_keys: Option<u64>,
}
```

### 5.2 Higher-level traits

```rust
/// Blob store for values larger than the engine's inline limit.
pub trait BlobStore: Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;
    type Reader: std::io::Read + Send;
    type Writer: std::io::Write + Send;

    fn put(&self, id: &[u8], reader: &mut dyn std::io::Read) -> Result<u64, Self::Error>;
    fn get(&self, id: &[u8]) -> Result<Self::Reader, Self::Error>;
    fn delete(&self, id: &[u8]) -> Result<(), Self::Error>;
}

/// Column-oriented engine for analytics workloads.
pub trait ColumnarEngine: Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;

    fn ingest(&self, batch: arrow::record_batch::RecordBatch) -> Result<(), Self::Error>;
    fn scan(&self, projection: &[&str], filter: &Predicate)
        -> Result<arrow::record_batch::RecordBatch, Self::Error>;
}

/// Engine that supports secondary indexes.
pub trait IndexedEngine: Engine {
    type IndexId;

    fn create_index(&self, name: &str, columns: &[&str]) -> Result<Self::IndexId, Self::Error>;
    fn drop_index(&self, id: Self::IndexId) -> Result<(), Self::Error>;
}
```

---

## 6. Implementation phases

The roadmap is intentionally sequential for the first three phases. Phases 4 and
beyond can run in parallel once the foundations are solid.

### Phase 0 — Shared infrastructure (1–2 weeks)

Crates to create:

* `storage-traits`
* `storage-testkit` (conformance tests, property generators, fault injection)
* `storage-util` (varint, bloom filter, checksums, file utilities)

Deliverables:

* Stable trait API.
* A conformance test harness that every engine must pass.
* Property-based tests for key/value boundaries.

### Phase 1 — Foundations (3–4 weeks)

1. **`storage-memory`**  
   * In-memory `BTreeMap` engine.  
   * Used for tests, caches, and as a reference implementation.  
   * Must pass the full conformance suite.

2. **`storage-wal`**  
   * Durable append-only log with checksums.  
   * Segment rotation and truncation.  
   * Recovery: replay from last checkpoint.

3. **`storage-kv` — LSM-tree**  
   * MemTable (use the existing lock-free skip-map design from `.doc/`).  
   * SSTable format with block index and Bloom filter.  
   * Leveled compaction.  
   * Crash recovery via WAL.  
   * Snapshot reads with MVCC.

4. **`storage-kv` — B-tree**  
   * Page-based B+ tree with copy-on-write or latch coupling.  
   * Optimized for read-heavy / range-scan workloads.  
   * Same trait API as the LSM engine.

By the end of Phase 1 you have two production-grade KV engines that are
swappable behind one trait.

### Phase 2 — Structure and analytics (4–6 weeks)

1. **`storage-columnar`**  
   * In-memory layout via Apache Arrow.  
   * On-disk persistence via Apache Parquet.  
   * Predicate pushdown using column statistics.  
   * Partition pruning.

2. **`storage-index`**  
   * Secondary index abstractions.  
   * B-tree indexes for range queries.  
   * Inverted index building blocks for search.  
   * HNSW/IVF index building blocks for vectors.

3. **`storage-metastore`**  
   * Catalog of tables, columns, partitions, indexes.  
   * Stored in the KV engine.

### Phase 3 — Specialized engines (6–10 weeks)

Each engine is built by composing traits and lower-level crates:

| Engine | Built on | Key challenges |
|--------|----------|----------------|
| `storage-document` | KV + columnar + JSON path index | Schema evolution, nested indexing |
| `storage-graph` | KV + adjacency indexes | Traversal locality, property indexes |
| `storage-time-series` | Columnar + specialized compression | Retention, downsampling, high cardinality |
| `storage-vector` | ANN index crate + KV | Distance metrics, recall vs. latency |
| `storage-search` | Inverted index + tokenizer | Ranking, phrase queries, stemming |
| `storage-object` | Chunked blob store + checksums | Large streams, deduplication, erasure coding |

### Phase 4 — Platform glue (4–6 weeks)

1. **`storage-cache`**  
   * Multi-tier cache: RAM → NVMe → object storage.  
   * Eviction policies (LRU, LFU, W-TinyLFU).

2. **`storage-federation`**  
   * Query planner that picks the right engine per access pattern.  
   * Push down filters/projection to the underlying engine.

3. **Replication hooks**  
   * Trait for a replicable engine.  
   * Integration with a future `consensus` crate.

---

## 7. Correctness and testing strategy

### 7.1 Conformance test suite

Every engine implementation runs the same tests:

```rust
// storage-testkit/src/conformance.rs
pub fn run_conformance_tests<E: Engine>(factory: impl Fn() -> E) {
    // Basic CRUD
    test_put_get_delete(&factory);
    test_overwrite(&factory);
    test_empty_key_value(&factory);
    test_large_value(&factory);

    // Ordering and scans
    test_scan_order(&factory);
    test_scan_prefix(&factory);
    test_seek(&factory);

    // Transactions
    test_commit(&factory);
    test_rollback(&factory);
    test_isolation(&factory);

    // Durability / recovery
    test_crash_recovery(&factory);
}
```

### 7.2 Property-based testing

Use `proptest` or `quickcheck` for:

* Random operation sequences followed by recovery.
* Arbitrary keys/values including Unicode, zero bytes, and max-size values.
* Concurrent operation interleavings.

### 7.3 Failure injection

Abstract the file system so tests can inject:

* `fsync` failures,
* partial writes,
* disk-full errors,
* power-loss at random points.

Use tools such as `fail-rs` or a custom `FileSystem` trait.

### 7.4 Model checking (for critical paths)

For lock-free structures like the skip-map, use:

* `loom` for concurrency model checking in Rust,
* `shuttle` for deterministic scheduling exploration.

### 7.5 Benchmarks

| Workload | Tool | Engines |
|----------|------|---------|
| OLTP | YCSB A/B/C/D/E/F | KV engines |
| Transactions | TPC-C | KV + transaction layer |
| Analytics | TPC-H | Columnar + federation |
| Time-series | TSBS | Time-series engine |
| Graph | LDBC SNB | Graph engine |
| Vector | ANN-Benchmarks | Vector engine |

---

## 8. Production-readiness checklist

Before any engine is marked production-ready, it must satisfy:

- [ ] Passes the full conformance suite.
- [ ] Property-based tests run for at least 24 hours without failure.
- [ ] Crash-recovery tests pass with power-loss injected at every WAL boundary.
- [ ] Benchmarks are within 3× of RocksDB/Parquet/SQLite baseline for the same
      workload class.
- [ ] Memory usage is bounded and documented.
- [ ] Metrics are exported (op latency, throughput, disk usage, cache hit rate).
- [ ] Backups and point-in-time recovery are possible.
- [ ] API is versioned and migrations are documented.
- [ ] All public items have doc comments and examples.
- [ ] Miri / Loom clean for any `unsafe` code.

---

## 9. Key references and sources

The following materials are the canonical background for the engines in this
plan. They should be studied in the order listed.

### Books

* Martin Kleppmann, *Designing Data-Intensive Applications* (O'Reilly, 2017)  
  Chapters 3 (storage/retrieval), 7 (transactions), 8 (distributed consistency),
  9 (consensus), 11 (stream processing).

* Abraham Silberschatz et al., *Database System Concepts*  
  B-trees, query processing, transactions, concurrency control.

* Maurice Herlihy & Nir Shavit, *The Art of Multiprocessor Programming*  
  Lock-free data structures, memory models.

### LSM-trees and KV engines

* Patrick O'Neil et al., "The Log-Structured Merge-Tree" (1996)  
  Original LSM paper.

* RocksDB wiki and source: <https://github.com/facebook/rocksdb/wiki>  
  Production LSM implementation.

* LevelDB source: <https://github.com/google/leveldb>  
  Minimal reference LSM.

* mini-lsm course: <https://skyzh.github.io/mini-lsm/> and
  <https://github.com/skyzh/mini-lsm>  
  Step-by-step Rust LSM implementation.

* "How to Build an LSM Tree Storage Engine from Scratch"  
  <https://www.freecodecamp.org/news/build-an-lsm-tree-storage-engine-from-scratch-handbook/>

### B-trees

* Douglas Comer, "The Ubiquitous B-Tree" (ACM Computing Surveys, 1979).

* Goetz Graefe, "Modern B-Tree Techniques" (Foundations and Trends in
  Databases, 2011).

* SQLite database file format: <https://www.sqlite.org/fileformat.html>

### Columnar storage

* Apache Arrow specification: <https://arrow.apache.org/docs/format/Columnar.html>

* Apache Parquet format: <https://parquet.apache.org/docs/file-format/>
  and Thrift definitions: <https://github.com/apache/parquet-format>

* "Dremel: Interactive Analysis of Web-Scale Datasets" (Google, 2010)  
  Nested columnar shredding.

* "MonetDB/X100: Hyper-Pipelining Query Execution" (Boncz et al., 2005)  
  Vectorized execution and columnar processing.

### Graph storage

* "The Neo4j Graph Database" book and source.

* "The GStore System" and RDF-3X papers for graph indexing.

### Time-series

* "Gorilla: A Fast, Scalable, In-Memory Time Series Database" (Facebook, 2015).

* "InfluxDB IOx" design documents and source.

* Time Series Benchmark Suite (TSBS): <https://github.com/timescale/tsbs>

### Vector search

* "Efficient and Robust Approximate Nearest Neighbor Search Using Hierarchical
  Navigable Small World Graphs" (Malkov & Yashunin, 2018).

* "Product Quantization for Nearest Neighbor Search" (Jégou et al., 2011).

* ANN-Benchmarks: <https://github.com/erikbern/ann-benchmarks>

### Consensus and replication (future phases)

* Diego Ongaro & John Ousterhout, "In Search of an Understandable Consensus
  Algorithm" (Raft, 2014).

* Leslie Lamport, "Paxos Made Simple" (2001).

* "Viewstamped Replication Revisited" (Liskov & Cowling, 2012).

### Rust-specific resources

* `rust-lang/rust` on `std::sync::atomic` and memory ordering.
* `tokio-rs/loom` for concurrency testing.
* `rust-fuzz/afl.rs` and `bluss/proptest` for property-based fuzzing.

---

## 10. First concrete milestones

This week:

1. Merge or adapt the existing lock-free skip-map from `.doc/skipmap.rs` into a
   `storage-memory` crate.
2. Define `storage-traits` with the traits above.
3. Write a conformance test harness that fails for the empty
   `storage_engines` crate and passes once a memory engine is implemented.

Next two weeks:

4. Implement `storage-wal`.
5. Implement an LSM-tree in `storage-kv` using the skip-map as the MemTable.
6. Run the conformance suite, property tests, and crash-recovery tests against
   both engines.

---

## 11. Decision log

| Decision | Rationale |
|----------|-----------|
| Storage before consensus | Consensus needs a durable local log and state machine. |
| Trait-first design | Allows callers to swap engines without code changes. |
| Separate crates per engine | Keeps compile times low and boundaries clean. |
| LSM + B-tree both required | LSM for write-heavy, B-tree for read-heavy/range workloads. |
| Conformance suite shared | Forces every engine to meet the same contract. |
| Columnar built on Arrow/Parquet | Reuses battle-tested formats; avoids format invention. |

---

*Last updated: 2026-07-15*
