# `storage-columnar` Production Readiness Checklist

**Engine type:** Columnar table store with partitioning, compaction, and snapshots  
**Status:** ✅ Signed off for production use as an embedded columnar engine  
**Last reviewed:** 2026-07-18

---

## 1. Durability

| # | Requirement | Evidence | Status |
|---|-------------|----------|--------|
| 1.1 | `sync_on_flush` fsyncs WAL, volumes, and table directory | `src/engine.rs` `ingest`, `sync` | ✅ |
| 1.2 | Atomic metadata updates via snapshot + rename | `src/manifest.rs` | ✅ |
| 1.3 | Crash recovery replays unflushed WAL | `src/engine.rs` `recover` | ✅ |

## 2. Correctness

| # | Requirement | Evidence | Status |
|---|-------------|----------|--------|
| 2.1 | Typed min/max stats per column | `src/manifest.rs` `ColumnStats` enum | ✅ |
| 2.2 | Stats pruning uses logical type comparison | `src/predicate.rs` | ✅ |
| 2.3 | Schema evolution validated | `tests/schema_evolution.rs` | ✅ |
| 2.4 | Null handling correct | `tests/null_handling.rs` | ✅ |
| 2.5 | Projection pushdown correct | `tests/projection.rs` | ✅ |
| 2.6 | Predicate pushdown correct | `tests/predicate_pushdown.rs` | ✅ |
| 2.7 | Corrupt snapshot reported, not silently ignored | `src/engine.rs` `recover` / `src/error.rs` | ✅ |

## 3. Compaction

| # | Requirement | Evidence | Status |
|---|-------------|----------|--------|
| 3.1 | Background compaction runs on dedicated thread | `src/engine.rs` + `src/compaction.rs` | ✅ |
| 3.2 | Compaction does not block ingest | channel-triggered worker | ✅ |
| 3.3 | `target_file_size` splits oversized output | `src/compaction.rs` planner / `src/options.rs` | ✅ |
| 3.4 | Compaction tested for correctness | `tests/compaction.rs` | ✅ |

## 4. Testing

| # | Test | Location | Status |
|---|------|----------|--------|
| 4.1 | Basic round-trip | `tests/basic_roundtrip.rs` | ✅ |
| 4.2 | Compaction | `tests/compaction.rs` | ✅ |
| 4.3 | Concurrency | `tests/concurrency.rs` | ✅ |
| 4.4 | Crash recovery | `tests/crash_recovery.rs` | ✅ |
| 4.5 | Null handling | `tests/null_handling.rs` | ✅ |
| 4.6 | Partitioning | `tests/partitioning.rs` | ✅ |
| 4.7 | Predicate pushdown | `tests/predicate_pushdown.rs` | ✅ |
| 4.8 | Projection | `tests/projection.rs` | ✅ |
| 4.9 | Recovery | `tests/recovery.rs` | ✅ |
| 4.10 | Schema evolution | `tests/schema_evolution.rs` | ✅ |
| 4.11 | Snapshot recovery | `tests/snapshot_recovery.rs` | ✅ |
| 4.12 | Stats pruning | `tests/stats_pruning.rs` | ✅ |

## 5. Operational concerns

| # | Requirement | Evidence | Status |
|---|-------------|----------|--------|
| 5.1 | Configurable sync/flush/compaction options | `src/options.rs` | ✅ |
| 5.2 | Graceful shutdown of background thread | `src/engine.rs` `close`/`Drop` | ✅ |
| 5.3 | Rich error variants | `src/error.rs` | ✅ |

## 6. Known limitations / follow-ups

- Async I/O is not yet implemented (sync only).
- Cloud object-store backend is out of scope for this crate.

## Sign-off

- **Tests pass:** `cargo test -p storage-columnar` ✅
- **Clippy clean:** `cargo clippy -p storage-columnar -- -D warnings` ✅
