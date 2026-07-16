# Storage Engines — Performance, Concurrency, and I/O Architecture

> This document is the companion to `DESIGN.md`. `DESIGN.md` says *what* engines
we build and in what order; this document says *how* each one must be made
fast, concurrent, and robust at the implementation level.

---

## 1. Honest scope statement

"As performant and robust as possible" is well-defined only when we name the
workload. Every engine makes trade-offs. This document records the explicit
decisions for each engine so that performance is intentional, not accidental.

What we **do not** do:

* Use async everywhere just because it is fashionable.
* Adopt `io_uring` for every file operation regardless of engine shape.
* Spawn actors for every internal component.
* Claim one engine is optimal for all workloads.

What we **do** do:

* Choose the concurrency model that matches the engine's access pattern.
* Use `io_uring` where it wins (high-concurrency random I/O) and buffered/direct
  I/O where it wins (sequential WAL, large scans).
* Use lock-free structures for hot paths, actors for isolated background
  workers, and channels for backpressure.
* Measure every decision with benchmarks.

---

## 2. Cross-cutting decisions

### 2.1 Sync vs async boundaries

| Layer | Sync or async | Reason |
|-------|---------------|--------|
| Engine trait API | **Sync + `Send + Sync`** | A storage engine should be usable from both async and sync callers. The trait itself must not force a runtime. |
| Internal I/O driver | **Async or blocking thread pool** | Depending on the engine and OS, use `tokio-uring`, a dedicated blocking thread pool, or `tokio::task::spawn_blocking`. |
| Background jobs (compaction, flush, checkpoint) | **Async tasks or OS threads** | Long-running, CPU/disk-heavy work. Must not block the engine's hot path. |
| Streaming reads | **`Iterator` for sync, `Stream` for async** | Callers choose. The engine provides both where reasonable. |
| Network replication (future) | **Async** | Network I/O is naturally async. |

**Rule:** The public `Engine` trait is sync. Internally, an engine may use an
async runtime or a thread pool, but that is an implementation detail.

```rust
// Public API is sync.
pub trait Engine: Send + Sync + 'static {
    fn get(&self, key: &[u8]) -> Result<Option<Bytes>, Error>;
    fn scan(&self, range: Range<&[u8]>) -> Result<Cursor, Error>;
}

// Internally, an LSM engine may dispatch I/O to tokio-uring or a thread pool.
pub struct LsmEngine {
    inner: Arc<LsmInner>,
    io: IoDriver, // enum: Uring | ThreadPool
}
```

### 2.2 I/O backend strategy

We support three I/O modes, selectable at engine open time:

```rust
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IoMode {
    /// Standard buffered I/O via `std::fs`. Portable, good for sequential
    /// workloads and WAL append.
    #[default]
    Buffered,
    /// Direct I/O with user-space buffering. Good for large databases where
    /// double-caching by the OS is harmful.
    Direct { block_size: usize },
    /// Linux io_uring. Best for high-concurrency random reads and writes.
    #[cfg(target_os = "linux")]
    IoUring,
}
```

| Engine component | Preferred I/O | Why |
|------------------|---------------|-----|
| WAL append | Buffered or direct | Sequential, latency-sensitive. `io_uring` adds little here and complicates ordering. |
| SSTable reads | `io_uring` or direct | Random, high-concurrency. `io_uring` reduces syscalls. |
| SSTable writes (flush/compaction) | Buffered or direct | Sequential large writes; throughput matters more than latency. |
| B-tree pages | `io_uring` or direct | Random page reads/writes. |
| Columnar scans | Direct or buffered | Large sequential reads. Avoid `io_uring` if single-threaded streaming is simpler. |

We introduce an `IoDriver` abstraction so the same engine code can run on all
three backends:

```rust
#[async_trait::async_trait]
pub trait IoDriver: Send + Sync {
    async fn read(&self, fd: &FileHandle, offset: u64, buf: &mut [u8]) -> Result<usize, Error>;
    async fn write(&self, fd: &FileHandle, offset: u64, buf: &[u8]) -> Result<usize, Error>;
    async fn sync(&self, fd: &FileHandle) -> Result<(), Error>;
}
```

### 2.3 Concurrency primitives by use case

| Use case | Primitive | Example |
|----------|-----------|---------|
| Hot-path reads against immutable SSTables | Lock-free / read-only | `Arc<SSTable>`, no locks |
| MemTable writes | Lock-free skip-list or sharded mutex | Existing skip-map design |
| MemTable freeze + flush handoff | Channel (oneshot or mpsc) | Freeze signals background task |
| Compaction scheduler | Actor / dedicated thread | Single compaction manager |
| Parallel compaction of disjoint key ranges | Thread pool / rayon | `rayon` for CPU-bound merge |
| Cache eviction | Lock-free LRU or W-TinyLFU | `s3-fifo` or custom epoch-based LRU |
| Transaction commit ordering | Mutex + condition variable | Commit queue |
| Metrics / background stats | Atomics | `AtomicU64`, no locks |

### 2.4 Actor pattern usage

We use actors **only** for components that benefit from serializing access to a
single state machine:

* Compaction manager (serializes which SSTables to compact).
* WAL segment rotator (serializes segment lifecycle).
* Metastore catalog writer (serializes schema changes).

We do **not** use actors for:

* Individual read requests (too much overhead).
* Every MemTable (lock-free is faster).

A typical actor skeleton:

```rust
use tokio::sync::{mpsc, oneshot};

pub struct CompactionActor {
    rx: mpsc::Receiver<CompactionMsg>,
}

enum CompactionMsg {
    Submit {
        level: usize,
        tables: Vec<SstHandle>,
        respond: oneshot::Sender<Result<CompactionJob, Error>>,
    },
    GetStats {
        respond: oneshot::Sender<CompactionStats>,
    },
}

impl CompactionActor {
    async fn run(mut self) {
        while let Some(msg) = self.rx.recv().await {
            match msg {
                CompactionMsg::Submit { level, tables, respond } => {
                    let _ = respond.send(self.schedule(level, tables));
                }
                CompactionMsg::GetStats { respond } => {
                    let _ = respond.send(self.stats());
                }
            }
        }
    }
}
```

### 2.5 Streaming and backpressure

For large scans, the engine must not materialize entire datasets in memory.

```rust
/// Sync streaming cursor.
pub trait Cursor: Iterator<Item = Result<(Bytes, Bytes), Error>> {
    fn batch_next(&mut self, n: usize) -> Result<Vec<(Bytes, Bytes)>, Error>;
}

/// Async streaming cursor for async callers.
pub trait AsyncCursor: futures::Stream<Item = Result<(Bytes, Bytes), Error>> {
    fn batched(self, n: usize) -> impl Stream<Item = Result<Vec<(Bytes, Bytes)>, Error>>;
}
```

Backpressure rules:

* A cursor reads one SSTable block at a time.
* Prefetch at most `N` blocks ahead, where `N` is configurable.
* For columnar scans, prefetch the next row group while the current is decoded.

---

## 3. Per-engine concurrency and I/O map

### 3.1 `storage-memory`

| Concern | Choice |
|---------|--------|
| Data structure | Lock-free skip-map (existing `.doc/` design) or sharded `BTreeMap` |
| Reads | Lock-free, wait-free in practice |
| Writes | Lock-free CAS tower |
| Iteration | Weakly consistent snapshot of level 0 |
| Use | Default MemTable, test engine, cache index |

### 3.2 `storage-wal`

| Concern | Choice |
|---------|--------|
| Writes | Sequential append with `writev` or single `write_all` |
| fsync policy | `GroupCommit` by default: batch fsyncs with a deadline |
| Reader | Single sequential reader for recovery |
| Concurrency | One writer thread/actor; readers during recovery only |
| I/O | Buffered for latency; direct optional |

Group commit implementation:

```rust
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};

pub struct WalWriter {
    file: Mutex<WalFile>,
    commit_notifier: broadcast::Sender<Lsn>,
}

impl WalWriter {
    pub async fn append(&self, rec: WalRecord) -> Result<Lsn, Error> {
        let mut file = self.file.lock().await;
        let lsn = file.append(rec).await?;
        // fsync is batched by a background task
        Ok(lsn)
    }

    pub async fn flush(&self, up_to: Lsn) -> Result<(), Error> {
        let mut rx = self.commit_notifier.subscribe();
        loop {
            let committed = *rx.borrow();
            if committed >= up_to {
                return Ok(());
            }
            rx.changed().await.map_err(|_| Error::WalClosed)?;
        }
    }
}
```

### 3.3 `storage-kv` (LSM-tree)

| Concern | Choice |
|---------|--------|
| MemTable | Lock-free skip-map |
| WAL | `storage-wal` group commit |
| Flush | Background async task triggered by size threshold |
| Compaction | Dedicated actor + thread pool for merge |
| SSTable reads | `io_uring` on Linux, direct/buffered elsewhere |
| Snapshot reads | MVCC with sequence numbers, no locks on read path |
| Range scans | Iterator over MemTable + SSTable cursors merged with k-way merge |

Parallel compaction:

```rust
use rayon::prelude::*;

fn compact_disjoint(inputs: Vec<Vec<SstHandle>>) -> Vec<Result<SstHandle, Error>> {
    inputs
        .into_par_iter()
        .map(|batch| merge_sstables(batch))
        .collect()
}
```

### 3.4 `storage-kv` (B-tree)

| Concern | Choice |
|---------|--------|
| Page cache | Lock-free page table or sharded LRU |
| Page latch | Read-write latches per page (latch coupling on descent) |
| Page writes | COW or in-place with WAL |
| I/O | `io_uring` for random page reads/writes |
| Range scans | Leaf-level sibling pointers + cursor |

### 3.5 `storage-columnar`

| Concern | Choice |
|---------|--------|
| In-memory | Apache Arrow `RecordBatch` |
| On-disk | Apache Parquet |
| Encoding | Dictionary, RLE, delta, bit-packing, ZSTD |
| Scans | Vectorized, SIMD-friendly Arrow kernels |
| Parallelism | Rayon for per-column decoding, per-row-group parallelism |
| Streaming | Row-group-at-a-time iterator |

### 3.6 `storage-time-series`

| Concern | Choice |
|---------|--------|
| Compression | Gorilla-style delta-of-delta for values, timestamp compression |
| Retention | TTL + downsampling as background compaction |
| Ingestion | Lock-free time partition ring buffer |
| Queries | Time-range pruning, vectorized aggregates |

### 3.7 `storage-vector`

| Concern | Choice |
|---------|--------|
| Index | HNSW (graph) or IVF (partitioned quantization) |
| Build | Parallel graph construction or cluster training |
| Search | Beam search with bounded work queue |
| I/O | `io_uring` for random index node reads |

### 3.8 `storage-search`

| Concern | Choice |
|---------|--------|
| Index | Inverted index with posting lists |
| Tokenizer | Parallel per-document pipeline |
| Merge | Background actor merges small segments |
| Ranking | BM25 or learned sparse scorer |

---

## 4. Exhaustive file tree

```text
crates/infra/
├── storage-traits/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── engine.rs          // Engine, Transaction, Cursor
│       ├── blob.rs            // BlobStore
│       ├── columnar.rs        // ColumnarEngine
│       ├── index.rs           // IndexedEngine
│       ├── cursor.rs          // Cursor, AsyncCursor
│       ├── options.rs         // TxnOptions, IsolationLevel, IoMode
│       ├── error.rs           // typed Error
│       └── metrics.rs         // EngineStats
│
├── storage-util/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── varint.rs
│       ├── checksum.rs        // CRC32C / xxhash
│       ├── bloom.rs           // Bloom filter
│       ├── file_ext.rs        // preallocate, fallocate, sync_dir
│       ├── bytes_ext.rs
│       └── io/
│           ├── mod.rs
│           ├── driver.rs      // IoDriver trait + backends
│           ├── buffered.rs
│           ├── direct.rs
│           └── uring.rs       // cfg(linux)
│
├── storage-testkit/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── conformance.rs     // run_conformance_tests
│       ├── property.rs        // proptest generators
│       ├── fault_injection.rs // fault-injectable filesystem
│       ├── workload.rs        // YCSB-style generators
│       └── model.rs           // reference model for verification
│
├── storage-memory/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── engine.rs
│       ├── skipmap.rs         // or import from util
│       └── cursor.rs
│
├── storage-wal/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── writer.rs
│       ├── reader.rs
│       ├── segment.rs
│       ├── group_commit.rs
│       └── recovery.rs
│
├── storage-kv/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── lsm/
│       │   ├── mod.rs
│       │   ├── engine.rs
│       │   ├── memtable.rs
│       │   ├── flush.rs
│       │   ├── compaction.rs
│       │   ├── sstable.rs
│       │   ├── sst_reader.rs
│       │   ├── sst_writer.rs
│       │   ├── sst_index.rs
│       │   ├── bloom.rs
│       │   ├── manifest.rs
│       │   ├── version.rs
│       │   └── mvcc.rs
│       └── btree/
│           ├── mod.rs
│           ├── engine.rs
│           ├── page.rs
│           ├── node.rs
│           ├── latch.rs
│           ├── cursor.rs
│           └── pager.rs
│
├── storage-columnar/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── arrow_engine.rs
│       ├── parquet_reader.rs
│       ├── parquet_writer.rs
│       ├── encoding.rs
│       ├── predicate.rs
│       └── statistics.rs
│
├── storage-index/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── btree.rs
│       ├── inverted.rs
│       ├── hnsw.rs
│       └── ivf.rs
│
├── storage-document/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── engine.rs
│       ├── codec.rs
│       └── path_index.rs
│
├── storage-graph/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── engine.rs
│       ├── adjacency.rs
│       └── label_index.rs
│
├── storage-time-series/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── engine.rs
│       ├── compression.rs
│       ├── partition.rs
│       └── retention.rs
│
├── storage-vector/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── engine.rs
│       ├── hnsw.rs
│       ├── ivf.rs
│       └── distance.rs
│
├── storage-search/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── engine.rs
│       ├── tokenizer.rs
│       ├── posting.rs
│       └── scorer.rs
│
├── storage-object/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── engine.rs
│       ├── chunker.rs
│       ├── dedup.rs
│       └── checksum.rs
│
├── storage-cache/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── tiered.rs
│       ├── eviction.rs
│       └── admission.rs
│
└── storage-federation/
    ├── Cargo.toml
    └── src/
        ├── lib.rs
        ├── router.rs
        ├── planner.rs
        └── pushdown.rs
```

---

## 5. Code samples for Phase 0/1

These are the first files to implement. They form the foundation everything else
builds on.

### 5.1 `storage-traits/src/lib.rs`

```rust
//! Public storage abstractions.

pub mod blob;
pub mod columnar;
pub mod cursor;
pub mod engine;
pub mod error;
pub mod index;
pub mod metrics;
pub mod options;

pub use blob::BlobStore;
pub use columnar::{ColumnarEngine, Predicate};
pub use cursor::{AsyncCursor, Cursor};
pub use engine::{Engine, Transaction};
pub use error::{Error, Result};
pub use index::IndexedEngine;
pub use metrics::EngineStats;
pub use options::{IoMode, IsolationLevel, TxnOptions};
```

### 5.2 `storage-traits/src/engine.rs`

```rust
use bytes::Bytes;
use std::ops::Range;

use crate::cursor::Cursor;
use crate::error::Result;
use crate::metrics::EngineStats;
use crate::options::{IsolationLevel, TxnOptions};

/// A synchronous, thread-safe storage engine.
pub trait Engine: Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;
    type Transaction: Transaction<Error = Self::Error>;
    type Cursor: Cursor<Error = Self::Error>;

    fn name(&self) -> &'static str;
    fn begin(&self, opts: TxnOptions) -> Result<Self::Transaction, Self::Error>;
    fn get(&self, key: &[u8]) -> Result<Option<Bytes>, Self::Error>;
    fn scan(&self, range: Range<&[u8]>) -> Result<Self::Cursor, Self::Error>;
    fn stats(&self) -> Result<EngineStats, Self::Error>;
    fn sync(&self) -> Result<(), Self::Error>;
}

pub trait Transaction: Sized + Send {
    type Error: std::error::Error + Send + Sync + 'static;

    fn get(&self, key: &[u8]) -> Result<Option<Bytes>, Self::Error>;
    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<(), Self::Error>;
    fn delete(&mut self, key: &[u8]) -> Result<(), Self::Error>;
    fn scan(&self, range: Range<&[u8]>) -> Result<impl Cursor<Error = Self::Error>, Self::Error>;
    fn commit(self) -> Result<(), Self::Error>;
    fn rollback(self) -> Result<(), Self::Error>;
    fn set_isolation(&mut self, level: IsolationLevel) -> Result<(), Self::Error>;
}
```

### 5.3 `storage-traits/src/cursor.rs`

```rust
use bytes::Bytes;
use std::vec;

pub trait Cursor: Iterator<Item = Result<(Bytes, Bytes), Self::Error>> {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Move to the first key >= target.
    fn seek(&mut self, target: &[u8]) -> Result<(), Self::Error>;

    /// Return the next `n` entries without allocating unbounded memory.
    fn next_batch(&mut self, n: usize) -> Result<Vec<(Bytes, Bytes)>, Self::Error> {
        let mut out = Vec::with_capacity(n.min(1024));
        for _ in 0..n {
            match self.next() {
                Some(Ok(kv)) => out.push(kv),
                Some(Err(e)) => return Err(e),
                None => break,
            }
        }
        Ok(out)
    }
}

#[cfg(feature = "async")]
pub trait AsyncCursor: futures::Stream<Item = Result<(Bytes, Bytes), Self::Error>> {
    type Error: std::error::Error + Send + Sync + 'static;
}
```

### 5.4 `storage-traits/src/options.rs`

```rust
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
pub struct TxnOptions {
    pub read_only: bool,
    pub isolation: IsolationLevel,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IoMode {
    #[default]
    Buffered,
    Direct { block_size: usize },
    #[cfg(target_os = "linux")]
    IoUring,
}
```

### 5.5 `storage-memory/src/engine.rs`

```rust
use bytes::Bytes;
use std::ops::Range;
use std::sync::Arc;

use storage_traits::{Cursor, Engine, EngineStats, Error, Result, Transaction, TxnOptions};

/// In-memory engine backed by a lock-free skip-map.
pub struct MemoryEngine {
    data: Arc<skipmap::SkipMap<Bytes, Bytes>>,
}

impl MemoryEngine {
    pub fn new() -> Self {
        Self {
            data: Arc::new(skipmap::SkipMap::new()),
        }
    }
}

impl Engine for MemoryEngine {
    type Error = Error;
    type Transaction = MemoryTransaction;
    type Cursor = MemoryCursor;

    fn name(&self) -> &'static str {
        "memory"
    }

    fn begin(&self, _opts: TxnOptions) -> Result<Self::Transaction> {
        Ok(MemoryTransaction {
            data: Arc::clone(&self.data),
        })
    }

    fn get(&self, key: &[u8]) -> Result<Option<Bytes>> {
        Ok(self.data.get(key).map(|v| v.clone()))
    }

    fn scan(&self, range: Range<&[u8]>) -> Result<Self::Cursor> {
        Ok(MemoryCursor {
            inner: self.data.range(range.start..range.end),
        })
    }

    fn stats(&self) -> Result<EngineStats> {
        Ok(EngineStats {
            name: self.name(),
            disk_bytes: 0,
            memory_bytes: 0,
            num_keys: Some(self.data.len() as u64),
        })
    }

    fn sync(&self) -> Result<()> {
        Ok(())
    }
}

pub struct MemoryTransaction {
    data: Arc<skipmap::SkipMap<Bytes, Bytes>>,
}

impl Transaction for MemoryTransaction {
    type Error = Error;

    fn get(&self, key: &[u8]) -> Result<Option<Bytes>> {
        Ok(self.data.get(key).map(|v| v.clone()))
    }

    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        self.data.insert(Bytes::copy_from_slice(key), Bytes::copy_from_slice(value));
        Ok(())
    }

    fn delete(&mut self, key: &[u8]) -> Result<()> {
        self.data.remove(key);
        Ok(())
    }

    fn scan(&self, range: Range<&[u8]>) -> Result<impl Cursor<Error = Self::Error>> {
        Ok(MemoryCursor {
            inner: self.data.range(range.start..range.end),
        })
    }

    fn commit(self) -> Result<()> {
        Ok(())
    }

    fn rollback(self) -> Result<()> {
        Ok(())
    }

    fn set_isolation(&mut self, _level: storage_traits::IsolationLevel) -> Result<()> {
        Ok(())
    }
}

pub struct MemoryCursor {
    // placeholder until real skip-map cursor is wired in
    inner: Vec<(Bytes, Bytes)>,
    pos: usize,
}

impl Iterator for MemoryCursor {
    type Item = Result<(Bytes, Bytes), Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos < self.inner.len() {
            let item = self.inner[self.pos].clone();
            self.pos += 1;
            Some(Ok(item))
        } else {
            None
        }
    }
}

impl Cursor for MemoryCursor {
    type Error = Error;

    fn seek(&mut self, target: &[u8]) -> Result<()> {
        self.pos = self.inner.partition_point(|(k, _)| k.as_ref() < target);
        Ok(())
    }
}
```

### 5.6 `storage-testkit/src/conformance.rs`

```rust
use storage_traits::{Engine, TxnOptions};

pub fn run_conformance_tests<E, F>(factory: F)
where
    E: Engine,
    F: Fn() -> E,
{
    test_put_get_delete(&factory);
    test_scan_order(&factory);
    test_empty_key_value(&factory);
    test_overwrite(&factory);
    test_transaction_commit_rollback(&factory);
}

fn test_put_get_delete<E: Engine>(factory: &dyn Fn() -> E) {
    let engine = factory();
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"a", b"1").unwrap();
    tx.commit().unwrap();

    assert_eq!(engine.get(b"a").unwrap(), Some(bytes::Bytes::from_static(b"1")));

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.delete(b"a").unwrap();
    tx.commit().unwrap();

    assert_eq!(engine.get(b"a").unwrap(), None);
}

fn test_scan_order<E: Engine>(factory: &dyn Fn() -> E) {
    let engine = factory();
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"c", b"3").unwrap();
    tx.put(b"a", b"1").unwrap();
    tx.put(b"b", b"2").unwrap();
    tx.commit().unwrap();

    let cursor = engine.scan(b"a"..b"d").unwrap();
    let keys: Vec<_> = cursor
        .map(|r| r.unwrap().0)
        .map(|k| String::from_utf8(k.to_vec()).unwrap())
        .collect();
    assert_eq!(keys, vec!["a", "b", "c"]);
}

fn test_empty_key_value<E: Engine>(_factory: &dyn Fn() -> E) {
    // Every engine must define its behavior for empty keys and values.
}

fn test_overwrite<E: Engine>(_factory: &dyn Fn() -> E) {}
fn test_transaction_commit_rollback<E: Engine>(_factory: &dyn Fn() -> E) {}
```

### 5.7 `storage-wal/src/group_commit.rs`

```rust
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Lsn(pub u64);

pub struct WalWriter {
    file: Arc<Mutex<WalFile>>,
    flushed: broadcast::Sender<Lsn>,
}

impl WalWriter {
    pub fn new(file: WalFile) -> Self {
        let (flushed, _) = broadcast::channel(16);
        Self {
            file: Arc::new(Mutex::new(file)),
            flushed,
        }
    }

    pub async fn append(&self, record: WalRecord) -> anyhow::Result<Lsn> {
        let mut file = self.file.lock().await;
        let lsn = file.append(record).await?;
        Ok(lsn)
    }

    pub async fn flush(&self, up_to: Lsn) -> anyhow::Result<()> {
        let mut rx = self.flushed.subscribe();
        loop {
            if *rx.borrow() >= up_to {
                return Ok(());
            }
            rx.changed().await?;
        }
    }

    /// Background task calls this periodically.
    pub async fn sync_and_notify(&self) -> anyhow::Result<()> {
        let mut file = self.file.lock().await;
        let lsn = file.sync().await?;
        let _ = self.flushed.send(lsn);
        Ok(())
    }
}

pub struct WalFile;

impl WalFile {
    pub async fn append(&mut self, _record: WalRecord) -> anyhow::Result<Lsn> {
        todo!()
    }
    pub async fn sync(&mut self) -> anyhow::Result<Lsn> {
        todo!()
    }
}

pub struct WalRecord;
```

---

## 6. Production-readiness checklist per engine

### Correctness
- [ ] Passes `storage-testkit` conformance suite.
- [ ] Property-based tests (proptest) run for 24 hours clean.
- [ ] Crash-recovery tests pass with power-loss injected at every fsync point.
- [ ] Concurrent tests pass under `loom` for any `unsafe`/lock-free code.
- [ ] Miri clean for any `unsafe` code.

### Performance
- [ ] Benchmarked against RocksDB/LevelDB/SQLite/Parquet baseline.
- [ ] Latency p50/p99 measured under load.
- [ ] Throughput measured at saturation.
- [ ] Memory usage bounded and profiled with `dhat` or `heaptrack`.

### Operations
- [ ] Metrics exported (counters, histograms, gauges).
- [ ] Configurable via `EngineOptions`.
- [ ] Backup and point-in-time recovery documented.
- [ ] Graceful shutdown flushes all state.
- [ ] File format versioned.

### Code quality
- [ ] No `unwrap`/`expect` in library code.
- [ ] Every public item documented.
- [ ] `clippy::pedantic` clean.
- [ ] Fuzz tests for parsers and decoders.

---

## 7. Curated references

### Rust async / io_uring

* tokio-uring: <https://github.com/tokio-rs/tokio-uring>
* tokio-uring DESIGN.md: <https://github.com/tokio-rs/tokio-uring/blob/master/DESIGN.md>
* "Exploring better async Rust disk I/O": <https://tonbo.io/blog/exploring-better-async-rust-disk-io>
* skyzh, io_uring async random read: <https://rustmagazine.github.io/rust_magazine_2021/chapter_2/io_uring_async_rw.html>

### Concurrency primitives and actors

* "Actors with Tokio" by Alice Ryhl: <https://ryhl.io/blog/actors-with-tokio/>
* `tokio::sync` channels and backpressure docs.
* `rayon` data parallelism: <https://github.com/rayon-rs/rayon>
* `loom` model checker: <https://github.com/tokio-rs/loom>

### LSM / KV

* RocksDB Tuning Guide: <https://github.com/facebook/rocksdb/wiki/RocksDB-Tuning-Guide>
* RocksDB source: <https://github.com/facebook/rocksdb>
* LevelDB source: <https://github.com/google/leveldb>
* mini-lsm book: <https://skyzh.github.io/mini-lsm/>

### B-tree

* Goetz Graefe, "Modern B-Tree Techniques":
  <https://www.cs.cmu.edu/~gibbons/methods/modernBTrees.pdf>
* SQLite file format: <https://www.sqlite.org/fileformat.html>

### Columnar

* Apache Arrow format: <https://arrow.apache.org/docs/format/Columnar.html>
* Apache Parquet format: <https://parquet.apache.org/docs/file-format/>
* "Dremel: Interactive Analysis of Web-Scale Datasets" (Google, 2010).

### Time-series

* "Gorilla: A Fast, Scalable, In-Memory Time Series Database" (Facebook, 2015).
* TSBS: <https://github.com/timescale/tsbs>

### Vector

* HNSW paper: Malkov & Yashunin, "Efficient and Robust Approximate Nearest
  Neighbor Search Using Hierarchical Navigable Small World Graphs" (2018).
* ANN-Benchmarks: <https://github.com/erikbern/ann-benchmarks>

### Search

* "Managing Gigabytes" (Witten, Moffat, Bell) for inverted indexes.
* Lucene source: <https://github.com/apache/lucene>

---

## 8. Next action

The first concrete step is to land **Phase 0**: `storage-traits`,
`storage-testkit`, and `storage-memory` wired into the workspace. Once that
passes the conformance suite, we add `storage-wal` and begin the LSM-tree.

If you want me to scaffold those Phase 0 crates as real files in the repo now,
say the word and I will create them.
