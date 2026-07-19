# `storage-index` Production Readiness Checklist

**Engine type:** Durable secondary-index wrapper over any `Engine`  
**Status:** ✅ Signed off for production use as an embedded secondary-index engine  
**Last reviewed:** 2026-07-18

---

## 1. Durability

| # | Requirement | Evidence | Status |
|---|-------------|----------|--------|
| 1.1 | Index entries durably persisted via underlying engine | `IndexEngine` stores index records in `S` | ✅ |
| 1.2 | Primary + index updates are atomic within a transaction | `src/engine.rs` `apply_in_transaction` | ✅ |
| 1.3 | Index backfill writes all existing primary keys before index is queryable | `src/engine.rs` `backfill_index` | ✅ |
| 1.4 | Drop cleanup removes all index records | `src/engine.rs` `drop_index` | ✅ |

## 2. Correctness

| # | Requirement | Evidence | Status |
|---|-------------|----------|--------|
| 2.1 | Index key format preserves ordering and avoids collisions | `src/key.rs` (index-name prefix + encoded value + primary key) | ✅ |
| 2.2 | Duplicate index values map to distinct primary keys | key suffix includes primary key | ✅ |
| 2.3 | Range scans over index return primary keys in index order | `src/engine.rs` `scan_index` | ✅ |
| 2.4 | Point lookups on index return matching primary keys | `src/engine.rs` `get_by_index` | ✅ |
| 2.5 | Transactions see consistent primary + index state | transaction wraps both updates | ✅ |

## 3. Concurrency

| # | Requirement | Evidence | Status |
|---|-------------|----------|--------|
| 3.1 | Index metadata protected by mutex | `src/engine.rs` `IndexEngine::state` | ✅ |
| 3.2 | Concurrent reads use underlying engine snapshots | `S::snapshot` / transactions | ✅ |

## 4. Testing

| # | Test | Location | Status |
|---|------|----------|--------|
| 4.1 | Basic index round-trip | `tests/engine.rs` | ✅ |
| 4.2 | Index range scan | `tests/engine.rs` | ✅ |
| 4.3 | Transactional primary+index update | `tests/engine.rs` | ✅ |
| 4.4 | Backfill from existing data | `tests/engine.rs` | ✅ |
| 4.5 | Drop index cleanup | `tests/engine.rs` | ✅ |

## 5. Operational concerns

| # | Requirement | Evidence | Status |
|---|-------------|----------|--------|
| 5.1 | Implements `Engine` and `IndexedEngine` traits | `src/lib.rs` | ✅ |
| 5.2 | Works with any `Engine` backend (in-memory for tests, disk engines in prod) | generic `S: Engine` | ✅ |
| 5.3 | Rich error variants | `src/error.rs` | ✅ |

## 6. Known limitations / follow-ups

- Online index backfill on very large datasets may block; incremental backfill is a future enhancement.
- Multi-column composite indexes are not yet supported.
- Unique index constraints are not yet enforced.

## Sign-off

- **Tests pass:** `cargo test -p storage-index` (standalone) ✅
- **Clippy clean:** `cargo clippy -p storage-index -- -D warnings` ✅
- **Note:** Not yet integrated into the main workspace; pending `storage-kv` and `storage-btree` hardening.
