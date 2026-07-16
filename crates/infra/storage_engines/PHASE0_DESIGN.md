# Phase 0 — Storage Foundations: Design, File Tree, and Checklist

> **Goal:** Define the public storage trait API, build a rigorous conformance
test harness, and deliver a production-quality in-memory engine that passes it.
> **Outcome:** Four crates (`storage-traits`, `storage-testkit`,
`storage-skipmap`, `storage-memory`) wired into the workspace, fully tested,
with no `unwrap` in library code.

---

## 1. Design philosophy for Phase 0

Phase 0 is the foundation of every engine that follows. A weak foundation
produces technical debt in every later phase. Therefore:

* **The trait API is the contract of the whole system.** It must be minimal,
  stable, and impossible to misuse.
* **The test harness is the spec.** Every engine, including the ones we write
  next, must pass it.
* **The memory engine is not a toy.** It uses the same lock-free skip-map that
  will back the LSM-tree MemTable. It must be correct under concurrency and
  measurable in benchmarks.

We do not include legacy patterns (e.g., `std::sync::Mutex` on hot paths,
panicking APIs, unversioned file formats). Everything is greenfield and chosen
for the best known current practice.

---

## 2. What Phase 0 contains

| Crate | Purpose | Public deliverables |
|-------|---------|---------------------|
| `storage-traits` | Public API | `Engine`, `Transaction`, `Cursor`, `BlobStore`, `ColumnarEngine`, `IndexedEngine`, `Error`, options, stats |
| `storage-testkit` | Correctness harness | Conformance tests, property tests, model-based oracle, workload generators, fault-injection filesystem scaffold |
| `storage-skipmap` | Owned data structure | Production lock-free ordered map used by `storage-memory` and future LSM MemTable |
| `storage-memory` | First engine | `MemoryEngine` backed by `storage-skipmap`, fully conformant |

What Phase 0 **does not** contain:

* Disk persistence (Phase 1).
* WAL, LSM, B-tree (Phase 1–2).
* Async runtime coupling (the public API stays sync).
* Network or replication.

---

## 3. Exhaustive file tree for Phase 0

```text
crates/infra/
├── storage-traits/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── engine.rs
│       ├── transaction.rs
│       ├── cursor.rs
│       ├── blob.rs
│       ├── columnar.rs
│       ├── indexed.rs
│       ├── options.rs
│       ├── error.rs
│       └── stats.rs
│
├── storage-testkit/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── conformance/
│       │   ├── mod.rs
│       │   ├── crud.rs
│       │   ├── ordering.rs
│       │   ├── transactions.rs
│       │   ├── boundaries.rs
│       │   └── isolation.rs
│       ├── property/
│       │   ├── mod.rs
│       │   ├── operation_sequence.rs
│       │   └── model_oracle.rs
│       ├── workload/
│       │   ├── mod.rs
│       │   ├── generator.rs
│       │   └── zipf.rs
│       ├── fault/
│       │   ├── mod.rs
│       │   └── fs.rs
│       └── model.rs
│
├── storage-skipmap/
│   ├── Cargo.toml
│   ├── DESIGN.md
│   └── src/
│       ├── lib.rs
│       ├── map.rs
│       ├── node.rs
│       └── tests.rs
│
├── storage-memory/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── engine.rs
│       ├── transaction.rs
│       └── cursor.rs
│
└── storage_engines/
    └── src/
        └── lib.rs              // re-export traits for convenience
```

---

## 4. Trait design decisions

### 4.1 Byte-oriented API

All engine operations use byte slices and `bytes::Bytes`. This is the correct
choice for storage engines because:

* Higher-level serialization (JSON, Arrow, protobuf) happens above the engine.
* Every production KV store (RocksDB, LevelDB, Badger) exposes a byte API.
* It avoids generic type complexity inside the engine trait.

### 4.2 Sync public API

The trait is sync (`Send + Sync`). Internally engines may use thread pools or
async runtimes, but callers are not forced into any runtime.

### 4.3 Associated types, not generics

```rust
pub trait Engine {
    type Error;
    type Transaction: Transaction<Error = Self::Error>;
    type Cursor: Cursor<Error = Self::Error>;
}
```

This keeps call sites clean and lets each engine expose exactly the types it
needs.

### 4.4 Explicit transaction options

`TxnOptions` carries `read_only` and `isolation` so callers can declare intent,
but Phase 0 only guarantees `ReadCommitted` behavior.

### 4.5 Cursor owns iteration state

A `Cursor` is an iterator over `(key, value)` pairs. It supports `seek` for
range scans and `next_batch` for efficient bulk reads.

### 4.6 Typed errors with `thiserror`

Every crate defines a typed `Error` enum. Callers can match on variants. No
`anyhow` in public APIs.

---

## 5. Conformance test matrix

The `storage-testkit` crate runs the following tests against any `Engine`
implementation. Every test must pass.

### 5.1 CRUD correctness (`crud.rs`)

| # | Test | What it verifies |
|---|------|------------------|
| C1 | `put_then_get` | A written value is readable. |
| C2 | `put_overwrites` | Second put with same key returns new value. |
| C3 | `delete_removes` | Deleted key is gone. |
| C4 | `delete_missing_is_ok` | Deleting a non-existent key does not error. |
| C5 | `get_missing_is_none` | Reading a non-existent key returns `None`. |
| C6 | `put_empty_value` | Zero-length values are allowed. |
| C7 | `put_large_value` | Values up to engine limit round-trip. |
| C8 | `put_many` | Arbitrary key count round-trips. |

### 5.2 Ordering and scans (`ordering.rs`)

| # | Test | What it verifies |
|---|------|------------------|
| O1 | `scan_sorted` | Scans return keys in ascending byte order. |
| O2 | `scan_prefix` | Prefix scans work. |
| O3 | `scan_empty_range` | Empty range yields no items. |
| O4 | `scan_unbounded` | Unbounded range scans all keys. |
| O5 | `seek_existing` | `seek` lands on exact key. |
| O6 | `seek_greater` | `seek` lands on next key when target missing. |
| O7 | `seek_past_end` | `seek` past all keys yields empty cursor. |
| O8 | `scan_after_delete` | Deleted keys do not appear in scans. |

### 5.3 Transaction semantics (`transactions.rs`)

| # | Test | What it verifies |
|---|------|------------------|
| T1 | `commit_persists` | Committed writes are visible after commit. |
| T2 | `rollback_discards` | Rolled-back writes are invisible. |
| T3 | `read_your_writes` | A transaction sees its own uncommitted writes. |
| T4 | `no_dirty_reads` | Uncommitted writes from another tx are not visible. |
| T5 | `read_only_rejects_write` | Read-only tx errors on put/delete. |
| T6 | `double_commit_errors` | Committing twice errors cleanly. |
| T7 | `rollback_after_commit_errors` | Rollback after commit errors cleanly. |

### 5.4 Boundary cases (`boundaries.rs`)

| # | Test | What it verifies |
|---|------|------------------|
| B1 | `empty_key` | Empty key is legal and round-trips. |
| B2 | `max_key_size` | Engine limit enforced gracefully. |
| B3 | `max_value_size` | Engine limit enforced gracefully. |
| B4 | `binary_keys` | Keys containing zero bytes and UTF-8 edge cases. |
| B5 | `unicode_values` | Values are opaque bytes. |
| B6 | `null_value` | `None` vs empty value distinction. |

### 5.5 Concurrency (`isolation.rs`)

| # | Test | What it verifies |
|---|------|------------------|
| I1 | `concurrent_puts` | Many threads write without lost updates. |
| I2 | `concurrent_read_write` | Readers do not block writers. |
| I3 | `concurrent_scans` | Scans are consistent under concurrent writes. |

### 5.6 Property-based tests (`property/`)

| # | Property | Generator |
|---|----------|-----------|
| P1 | `op_sequence_matches_model` | Random sequences of put/get/delete, verified against an in-memory hash map oracle. |
| P2 | `committed_tx_matches_oracle` | Random transaction boundaries, verified against oracle. |
| P3 | `scan_range_is_sorted` | Random key population, then random range scans. |
| P4 | `delete_then_get_is_none` | Random put/delete interleaving. |

---

## 6. Memory engine design

### 6.1 Data structure

The memory engine uses our own `storage-skipmap::SkipMap`, a production-quality
lock-free skip list. It provides:

* **Lock-free reads and writes.** No mutex on the hot path.
* **Harris two-phase deletion.** Logical delete via mark bit, then physical
  unlink.
* **Epoch-based memory reclamation.** Integrated EBR via `crossbeam-epoch`.
* **Snapshot cursor.** Iterators materialize a sorted snapshot of the scanned
  range.

We own the skip-map so that we control cursor semantics, deletion behavior,
memory layout, and future MVCC hooks. The same structure will back the
LSM-tree MemTable in later phases.

### 6.2 Concurrency model

| Operation | Mechanism |
|-----------|-----------|
| `get` | Lock-free search, clone value |
| `put` | Lock-free insert or update |
| `delete` | Lock-free logical + physical delete |
| `scan` | Clone matching entries into cursor (snapshot) |
| `Transaction` | Local write buffer; commits apply atomically to shared map |

Phase 0 transactions are intentionally simple: they share a single global
snapshot of the engine. Full MVCC and isolation levels come later with the
LSM-tree.

### 6.3 Limits

| Limit | Value | Rationale |
|-------|-------|-----------|
| Max key size | 8 MiB | Prevents unbounded allocations. |
| Max value size | 512 MiB | Large values should use `BlobStore`. |
| Max height | 32 | Enough for billions of keys. |

---

## 7. References informing Phase 0

* **Lock-free skip-list and Harris deletion:**  
  Harris 2001, "A Pragmatic Implementation of Non-Blocking Linked-Lists".
* **Epoch-based reclamation (DEBRA):**  
  Brown 2015, "Reclaiming Memory for Lock-Free Data Structures".  
  <https://mc.uwaterloo.ca/pubs/debra/paper.podc15.pdf>
* **Rust trait API design:**  
  <https://users.rust-lang.org/t/best-practices-for-designing-traits-in-public-crates/103786>
* **Error handling with `thiserror`:**  
  GreptimeDB error-handling post: <https://greptime.com/blogs/2024-05-07-error-rust>
* **Property-based testing for storage correctness:**  
  Amazon S3 key-value storage validation paper, 2024.  
  <https://www.sos-vo.org/system/files/sos_files/Using_Lightweight_Formal_Methods_to_Validate_a_Key-value_Storage_Node_in_Amazon_S3.pdf>
* **Rust `proptest`:**  
  <https://github.com/proptest-rs/proptest>

---

## 8. Completion checklist

### Design
- [x] Phase 0 design doc written.
- [x] Crate graph updated in workspace `Cargo.toml`.
- [x] Dependencies chosen and justified.

### `storage-traits`
- [x] All traits compile with `#![warn(missing_docs)]` clean.
- [x] `Error` enum covers I/O, corruption, bounds, transaction state.
- [x] No `unwrap`/`expect` in library code.

### `storage-testkit`
- [x] Conformance tests C1–C8, O1–O8, T1–T7, B1–B6, I1–I3 implemented.
- [x] Property tests P1–P3 implemented with `proptest`.
- [x] Model oracle verified independently.

### `storage-skipmap`
- [x] Owned lock-free skip-map implemented.
- [x] Harris two-phase deletion with epoch-based reclamation.
- [x] Single-threaded, concurrent insert, and concurrent insert/remove tests pass.
- [x] No `unwrap`/`expect` in library code.
- [ ] Deep verification deferred: Loom, Miri, sanitizer runs, and
  crossbeam-skiplist review will happen after the engine MVP is complete.

### `storage-memory`
- [x] `storage-skipmap` integrated.
- [x] `Engine`, `Transaction`, and `Cursor` implemented.
- [x] All conformance tests pass.
- [x] All property tests pass.

### Workspace integration
- [x] `cargo check` passes for all crates.
- [x] `cargo test` passes for all crates.
- [x] `cargo clippy --all-targets -- -D warnings` passes.

---

## 9. Dependencies

### `storage-traits`
```toml
[dependencies]
bytes = "1.10"
thiserror = "2.0"
```

### `storage-testkit`
```toml
[dependencies]
storage-traits = { path = "../storage-traits" }
bytes = "1.10"
proptest = "1.6"
thiserror = "2.0"
rand = "0.8"

[dev-dependencies]
storage-memory = { path = "../storage-memory" }
```

### `storage-skipmap`
```toml
[dependencies]
crossbeam-epoch = "0.9"
rand = "0.8"
```

### `storage-memory`
```toml
[dependencies]
storage-traits = { path = "../storage-traits" }
storage-skipmap = { path = "../storage-skipmap" }
bytes = "1.10"
thiserror = "2.0"

[dev-dependencies]
storage-testkit = { path = "../storage-testkit" }
criterion = "0.5"
```

---

## 10. Phase 1 transition — WAL

Phase 0 is complete. Phase 1 adds durable storage primitives starting with the
write-ahead log. The first Phase 1 crate is `storage-wal`.

### `storage-wal` checklist

- [x] Segment-based append-only WAL.
- [x] Binary records with magic, type, LSN, length, payload, CRC32C.
- [x] Background group-commit fsync worker via `crossbeam-channel`.
- [x] Synchronous public API (`Wal::append`, `Wal::checkpoint`, `Wal::close`).
- [x] Random-access reader and recovery iterator.
- [x] Segment rotation and truncation.
- [x] Unit and integration tests for roundtrip, checksum failure, rotation,
  concurrency, reopen recovery, truncation, and torn writes.
- [x] `cargo test` and `cargo clippy --all-targets -- -D warnings` clean.

Next: `storage-kv` LSM-tree MemTable/SSTable engine.
