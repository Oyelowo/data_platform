# Phase 1 — Persistent KV: LSM-Tree MemTable, SSTable, and Compaction

> **Goal:** Add the first disk-persistent key-value engine to the platform. It
> is an LSM-tree with a WAL-backed MemTable, SSTables with Bloom filters, and
> leveled compaction.
> **Outcome:** A new `storage-kv` crate that implements the `storage-traits`
> `Engine` contract and passes basic integration tests plus crash-recovery
> property tests. Flush and compaction are synchronous in this first cut.

---

## 1. Design philosophy for Phase 1

Phase 1 builds directly on Phase 0:

* The **MemTable** reuses `storage-skipmap`, our owned lock-free skip-map.
* The **WAL** reuses `storage-wal`, our group-commit segment log.
* The public API stays **synchronous** and runtime-agnostic.
* All file formats are **versioned** and **checksum-protected**.
* There is no legacy compatibility; everything is greenfield.

The engine targets production correctness first, then throughput. We use the
best-known current practices from the LSM literature and from RocksDB,
CockroachDB Pebble, and LevelDB:

* Leveled compaction for predictable read and space amplification.
* Per-SSTable full Bloom filters with configurable bits-per-key.
* Block-based SSTables with restart intervals for prefix compression.
* Sequence-numbered internal keys for MVCC-style snapshots.
* A `MANIFEST` file for atomic metadata changes.
* **Synchronous flush and compaction** in this first cut; background threads are
  a planned performance optimization that does not change the format or recovery
  story.

Key references:

* [RocksDB Tuning Guide](https://github.com/facebook/rocksdb/wiki/RocksDB-Tuning-Guide)
* [RocksDB Compaction Wiki](https://github.com/facebook/rocksdb/wiki/Compaction)
* [CockroachDB Pebble `sstable` package](https://pkg.go.dev/github.com/cockroachdb/pebble/sstable)
* [LevelDB annotated source — SSTable meta block](https://stleox.github.io/leveldb-1.23-annotated/07-SSTable-meta-block/)
* [Survey of LSM-Tree based Indexes](https://arxiv.org/pdf/2402.10460.pdf)
* [LSM Trees in Adversarial Environments](https://arxiv.org/html/2502.08832v1)

---

## 2. What Phase 1 contains

| Crate | Purpose | Public deliverables |
|-------|---------|---------------------|
| `storage-wal` | Durable append log | Reused from Phase 0/1 transition; records KV writes |
| `storage-skipmap` | Lock-free ordered map | Reused as MemTable backing store |
| `storage-kv` | Persistent LSM-tree KV engine | `LsmEngine` implementing `Engine`, `Transaction`, `Cursor` |
| `storage-testkit` | Conformance + crash harness | Extended with crash-recovery and compaction property tests |

What Phase 1 **does not** contain:

* Column families (single CF only).
* Distributed replication or consensus.
* BlobDB-style value separation (all values inline in SSTable).
* Async runtime coupling.
* Tiered/universal compaction (deferred to Phase 2).

---

## 3. Exhaustive file tree for Phase 1

```text
crates/infra/
├── storage-wal/                 # existing, reused
│   ├── Cargo.toml
│   ├── DESIGN.md
│   └── src/...
│
├── storage-skipmap/             # existing, reused
│   ├── Cargo.toml
│   ├── DESIGN.md
│   └── src/...
│
├── storage-kv/                  # NEW
│   ├── Cargo.toml
│   ├── DESIGN.md
│   ├── src/
│   │   ├── lib.rs               # public exports, Error, Options
│   │   ├── engine.rs            # LsmEngine: impl Engine
│   │   ├── transaction.rs       # LsmTransaction: impl Transaction
│   │   ├── cursor.rs            # LsmCursor: impl Cursor
│   │   ├── memtable.rs          # MemTable wrapper around storage-skipmap
│   │   ├── immutable.rs         # ImmutableMemTable + flush scheduling
│   │   ├── version.rs           # Version: snapshot of levels + SST files
│   │   ├── version_set.rs       # VersionSet: level metadata, MANIFEST
│   │   ├── compaction.rs        # compaction picker + background worker
│   │   ├── sstable/
│   │   │   ├── mod.rs           # public sstable types
│   │   │   ├── builder.rs       # SSTable writer
│   │   │   ├── reader.rs        # SSTable reader + block cache hooks
│   │   │   ├── block.rs         # data block builder/iterator
│   │   │   ├── index.rs         # index block builder/iterator
│   │   │   ├── filter.rs        # Bloom filter block
│   │   │   └── format.rs        # Footer, block trailer, magic numbers
│   │   ├── wal.rs               # integration with storage-wal
│   │   ├── manifest.rs          # MANIFEST log of version edits
│   │   └── recovery.rs          # reopen: replay WAL, recover VersionSet
│   └── tests/
│       ├── engine_basic.rs      # put/get/delete/scan/reopen smoke tests
│       └── engine_recovery.rs   # crash-recovery and property-based tests
│
└── storage_engines/
    └── src/
        └── lib.rs               # re-export storage-kv
```

---

## 4. Internal architecture

```text
                       +----------------+
  write path  -------->|   LsmEngine    |
                       +--------+-------+
                                |
              +-----------------+-----------------+
              v                                   v
       +-------------+                    +-------------+
       |  MemTable   | (mutable)          |     WAL     |
       | storage-skip|                    | storage-wal |
       +------+------+                    +-------------+
              |
              | size >= write_buffer_size
              v
       +-------------+
       |   Flush     |------> SST L0 file
       |  (sync)     |
       +------+------+
              |
              | score > 1
              v
       +-------------+
       |  Compaction |------> merge Ln -> Ln+1
       |  (sync)     |
       +------+------+
              |
              v
       +-------------+
       | VersionSet  |<-----> MANIFEST
       | (levels)    |
       +-------------+
```

### 4.1 Write path

1. Serialize the key/value into a `WalRecord`.
2. Append to `storage-wal` with `Durability::Immediate`.
3. Apply to the mutable `MemTable`.
4. If MemTable exceeds `write_buffer_size`, freeze it and schedule a flush.

### 4.2 Read path

1. Search mutable MemTable (newest).
2. Search immutable MemTables in reverse freeze order.
3. Search L0 SSTables in reverse flush order.
4. Search L1..Lmax levels; each level is non-overlapping, so binary-search the
   file whose range contains the key.
5. Within an SSTable, consult the Bloom filter first, then the index block,
   then the data block.

### 4.3 Internal key format

```text
| user_key | sequence (u64 LE) |
```

The low byte of the sequence field stores the type (`Value` or `Delete`
tombstone); the high 56 bits store the sequence number. Sequence numbers
**increase** with newer writes. A snapshot with sequence `S` sees entries with
sequence `<= S`, choosing the largest such sequence.

### 4.4 MemTable

* Backed by `storage-skipmap`.
* Stores encoded internal keys.
* Provides a forward iterator used by flush.
* Memory usage tracked; when usage exceeds `write_buffer_size` the current
  MemTable is flushed synchronously to L0 and replaced with a fresh one.

### 4.5 Immutable MemTables

* **Deferred.** There is no immutable MemTable queue in this cut; flushes are
  synchronous and the engine blocks writers until the flush finishes.

### 4.6 SSTable format

A file is a sequence of blocks followed by a `Footer`:

```text
[ data block ]*
[ meta block: filter ]
[ meta index block ]
[ index block ]
[ footer: 48 bytes ]
```

Each block has a 5-byte trailer: CRC32 (4 bytes) + compression type (1 byte).

**Data block**

* Restart-point every `block_restart_interval` keys.
* Shared-prefix length / non-shared length / value length / delta key / value.
* Restart points stored as absolute offsets in the block.

**Index block**

* One entry per data block: `shortest_separator(last_key_in_block, first_key_in_next)`
  plus the block offset and size.

**Filter block**

* One Bloom filter for the whole SSTable, addressed from the meta index.
* Uses double hashing; bits-per-key configurable (default 10).

**Footer**

```text
| metaindex_handle | index_handle | version | magic |
```

### 4.7 VersionSet and MANIFEST

* `Version` is an immutable snapshot of level files.
* `VersionSet` owns the current `Version` and a write-ahead `MANIFEST` log.
* Each flush or compaction produces a `VersionEdit`:
  * deleted files (level, file_number)
  * new files (level, file metadata: number, size, smallest/largest key)
* Edits are appended to MANIFEST, then applied to the in-memory VersionSet.
* On reopen: read MANIFEST, replay edits to recover VersionSet.

### 4.8 Leveled compaction

* L0: tiered (multiple overlapping files) from flushes.
* L1..Lmax: leveled (one sorted run, files non-overlapping).
* Level size target: `L1 = max_bytes_for_level_base`; `Ln = Ln-1 * multiplier`.
* Compaction score per level = `total_size / target_size`.
* Pick the level with highest score > 1.
* For L0, also trigger when file count exceeds `level0_file_num_compaction_trigger`.
* Pick input files:
  * Ln: files with highest overlap ratio with Ln+1 (RocksDB
    `kMinOverlappingRatio`).
  * Ln+1: all files whose key range overlaps the Ln inputs.
* Output split into files of `target_file_size`.
* During compaction, drop keys shadowed by newer versions or tombstones that
  have reached the bottom level.

### 4.9 Recovery

1. Read `CURRENT` file to find the active MANIFEST.
2. Replay MANIFEST edits into a fresh VersionSet.
3. Replay WAL records starting from sequence 0.
   * Rebuild mutable MemTable.
   * Track the maximum sequence number seen.
4. Use the recovered maximum sequence number as the engine's `last_sequence`.
   (WAL truncation and immutable-MemTable flush during replay are deferred.)

---

## 5. Public API

```rust
use storage_kv::{LsmEngine, LsmOptions};
use storage_traits::{Engine, Transaction};

let engine = LsmEngine::open("/tmp/db", LsmOptions::default())?;
let mut tx = engine.begin_write()?;
tx.put(b"hello", b"world")?;
tx.commit()?;
```

### `LsmOptions`

| Field | Default | Description |
|-------|---------|-------------|
| `write_buffer_size` | 64 MiB | Mutable MemTable size limit |
| `max_write_buffer_number` | 3 | Max mutable + immutable memtables |
| `level0_file_num_compaction_trigger` | 4 | L0 files before compaction |
| `level0_slowdown_writes_trigger` | 12 | L0 files before write slowdown |
| `level0_stop_writes_trigger` | 20 | L0 files before write stall |
| `max_bytes_for_level_base` | 256 MiB | L1 target size |
| `max_bytes_for_level_multiplier` | 10 | Per-level size ratio |
| `target_file_size_base` | 64 MiB | L1 SST file size |
| `target_file_size_multiplier` | 1 | File size growth per level |
| `num_levels` | 7 | Max levels |
| `block_size` | 4 KiB | SST data block size |
| `block_restart_interval` | 16 | Restart points per block |
| `bloom_bits_per_key` | 10 | Bloom filter density |
| `compression` | None initially | Later: LZ4/Snappy |
| `wal_segment_size` | 64 MiB | Forwarded to `storage-wal` |

---

## 6. Correctness checklist

### 6.1 Writes

* [x] Every write is appended to WAL before applying to MemTable.
* [x] MemTable is flushed synchronously when it exceeds `write_buffer_size`.
* [ ] Write stalls when `level0_stop_writes_trigger` is hit. *(deferred)*
* [x] No lost updates on crash after `sync()` returns.

### 6.2 Reads

* [x] Point get returns the newest visible value.
* [x] Deleted keys return `None`.
* [x] Range scans return keys in ascending order.
* [ ] Prefix scans use bloom filters when beneficial. *(deferred)*
* [x] Read-your-writes within a transaction.
* [ ] Snapshot isolation for reads beyond `last_sequence`. *(deferred)*

### 6.3 SSTables

* [x] Builder produces sorted blocks.
* [x] Reader verifies block checksums.
* [x] Iterator handles restart points and prefix compression.
* [x] Bloom filter false-positive rate is bounded by bits-per-key.
* [x] Index block supports seek by key.

### 6.4 Compaction

* [x] Compaction preserves the latest visible version of each key.
* [x] Tombstones are dropped only at bottom-most level.
* [x] Level size targets drive compaction selection.
* [x] Synchronous compaction and reads do not conflict (single writer lock).
* [ ] Compaction failure leaves DB in consistent state. *(partial: MANIFEST is
  logged before applying edits; orphaned files are not yet cleaned)*

### 6.5 Recovery

* [x] Reopen replays MANIFEST edits into VersionSet.
* [x] Reopen replays WAL records.
* [ ] Torn WAL tail is truncated before replay. *(deferred)*
* [x] MANIFEST replay is idempotent (replays edits onto a fresh VersionSet).
* [ ] Missing SST files are detected and reported. *(deferred)*
* [ ] Crash after flush but before MANIFEST edit recovers correctly. *(deferred)*

---

## 7. Testing matrix

### 7.1 Unit tests

| File | Test | Verifies |
|------|------|----------|
| `sstable/block.rs` | block roundtrip, restart seek | prefix compression |
| `sstable/filter.rs` | bloom present/absent | false positive bound |
| `sstable/reader.rs` | builder -> reader -> get | full SST lifecycle |
| `internal_key.rs` | roundtrip, ordering | internal key encoding |
| `memtable.rs` | get/put/delete | MemTable behavior |

### 7.2 Integration tests

| Test | What it does |
|------|--------------|
| `engine_basic.rs` | put/get/delete/scan/reopen smoke tests |
| `engine_recovery.rs` | Random ops, sync, reopen, verify against BTree oracle |
| `engine_recovery.rs` | Overwrite keys enough to trigger compaction; verify latest value |

### 7.3 Property tests

* Random sequence of `put`/`delete` operations, sync, reopen, and compare both
  point gets and full scans to an in-memory BTree oracle.

---

## 8. Implementation order

1. Scaffold `crates/infra/storage-kv/` and add to workspace. ✅
2. Implement internal key encoding. ✅
3. Implement `MemTable` over `storage-skipmap`. ✅
4. Implement SSTable block builder/iterator. ✅
5. Implement Bloom filter block. ✅
6. Implement SSTable index block. ✅
7. Implement SSTable `Builder` and `Reader`. ✅
8. Implement `Version`, `VersionEdit`, `VersionSet`, `MANIFEST`. ✅
9. Wire WAL via `storage-wal`; implement recovery. ✅
10. Implement synchronous flush. ✅
11. Implement compaction picker and synchronous compaction. ✅
12. Implement `LsmEngine`, `LsmTransaction`, `LsmCursor`. ✅
13. Add basic integration and crash-recovery property tests. ✅
14. Run full workspace `cargo test` and `cargo clippy --all-targets -- -D warnings`.

---

## 9. Dependencies

```toml
[dependencies]
storage-traits = { path = "../storage-traits" }
storage-skipmap = { path = "../storage-skipmap" }
storage-wal = { path = "../storage-wal" }
bytes = "1.10"
thiserror = "2.0"
crc32c = "0.6"
serde = { version = "1.0", features = ["derive"] }
bincode = "1.3"
bitvec = "1.0"

[dev-dependencies]
storage-testkit = { path = "../storage-testkit" }
tempfile = "3.19"
proptest = "1.6"
```

---

## 10. Future work / deferred to Phase 2

* Tiered/Universal compaction option.
* Column families.
* Blob / value separation.
* Compression (LZ4/Snappy/Zstd).
* Block cache (LRU) and table cache.
* Prefix bloom filters and hash indexes.
* Multi-threaded compaction (subcompactions).
* Range deletion (range tombstones).
* Backup / checkpoint.
* Sanitizer, Miri, and Loom testing of recovery paths.
