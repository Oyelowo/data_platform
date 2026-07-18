//! Integration tests for the in-place B+ tree engine.

use std::io::{Seek, SeekFrom, Write};
use std::sync::Arc;
use std::thread;

use bytes::Bytes;
use storage_btree::{BtreeEngine, BtreeOptions};
use storage_traits::{Engine, Transaction, TxnOptions};
use tempfile::TempDir;

fn open_engine() -> (BtreeEngine, TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let engine = BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap();
    (engine, dir)
}

fn reopen_engine(dir: &TempDir) -> BtreeEngine {
    BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap()
}

fn small_page_options() -> BtreeOptions {
    BtreeOptions {
        page_size: 512,
        max_inline_value_size: 64,
        min_fill_percent: 50,
        min_cells: Some(1),
        cache_size: 0,
        max_value_size: 16 * 1024 * 1024,
        max_batch_ops: 10_000,
        ..Default::default()
    }
}

#[test]
fn basic_crud() {
    let (engine, _dir) = open_engine();

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"a", b"1").unwrap();
    tx.put(b"b", b"2").unwrap();
    tx.commit().unwrap();

    assert_eq!(engine.get(b"a").unwrap(), Some(Bytes::from_static(b"1")));
    assert_eq!(engine.get(b"b").unwrap(), Some(Bytes::from_static(b"2")));

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.delete(b"a").unwrap();
    tx.commit().unwrap();

    assert_eq!(engine.get(b"a").unwrap(), None);
    assert_eq!(engine.get(b"b").unwrap(), Some(Bytes::from_static(b"2")));
}

#[test]
fn empty_key_and_value() {
    let (engine, _dir) = open_engine();

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"", b"").unwrap();
    tx.commit().unwrap();

    assert_eq!(engine.get(b"").unwrap(), Some(Bytes::new()));
}

#[test]
fn overwrite_value() {
    let (engine, _dir) = open_engine();

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"k", b"v1").unwrap();
    tx.commit().unwrap();

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"k", b"v2").unwrap();
    tx.commit().unwrap();

    assert_eq!(engine.get(b"k").unwrap(), Some(Bytes::from_static(b"v2")));
}

#[test]
fn delete_missing_is_ok() {
    let (engine, _dir) = open_engine();
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.delete(b"missing").unwrap();
    tx.commit().unwrap();
    assert_eq!(engine.get(b"missing").unwrap(), None);
}

#[test]
fn scan_range_and_seek() {
    let (engine, _dir) = open_engine();

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    for i in 0..10u8 {
        tx.put(&[i], &[i + 100]).unwrap();
    }
    tx.commit().unwrap();

    let cursor = engine.scan(Some(&[3u8]), Some(&[7u8])).unwrap();
    let items: Vec<_> = cursor.map(|r| r.unwrap()).collect();
    assert_eq!(items.len(), 4);
    assert_eq!(items[0].0.as_ref(), &[3]);
    assert_eq!(items[3].0.as_ref(), &[6]);

    let mut cursor = engine.scan(None, None).unwrap();
    cursor.seek(&[5u8]).unwrap();
    let items: Vec<_> = cursor.map(|r| r.unwrap().0).collect();
    assert_eq!(items.len(), 5);
    assert_eq!(items[0].as_ref(), &[5]);
}

#[test]
fn scan_unbounded() {
    let (engine, _dir) = open_engine();

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"a", b"1").unwrap();
    tx.put(b"b", b"2").unwrap();
    tx.put(b"c", b"3").unwrap();
    tx.commit().unwrap();

    let cursor = engine.scan(None, None).unwrap();
    let keys: Vec<_> = cursor.map(|r| r.unwrap().0).collect();
    assert_eq!(
        keys,
        vec![
            Bytes::from_static(b"a"),
            Bytes::from_static(b"b"),
            Bytes::from_static(b"c")
        ]
    );
}

#[test]
fn large_overflow_value() {
    let (engine, _dir) = open_engine();

    let value = vec![0xABu8; 1_048_576];
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"large", &value).unwrap();
    tx.commit().unwrap();

    let read = engine.get(b"large").unwrap();
    assert_eq!(read, Some(Bytes::from(value)));
}

#[test]
fn reopen_replays_wal() {
    let (engine, dir) = open_engine();

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"a", b"1").unwrap();
    tx.put(b"b", b"2").unwrap();
    tx.commit().unwrap();

    drop(engine);

    let engine = reopen_engine(&dir);
    assert_eq!(engine.get(b"a").unwrap(), Some(Bytes::from_static(b"1")));
    assert_eq!(engine.get(b"b").unwrap(), Some(Bytes::from_static(b"2")));
}

#[test]
fn sync_truncates_wal_and_reopens() {
    let (engine, dir) = open_engine();

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"a", b"1").unwrap();
    tx.commit().unwrap();

    engine.sync().unwrap();
    drop(engine);

    let engine = reopen_engine(&dir);
    assert_eq!(engine.get(b"a").unwrap(), Some(Bytes::from_static(b"1")));
}

#[test]
fn read_your_writes() {
    let (engine, _dir) = open_engine();
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"a", b"1").unwrap();
    assert_eq!(tx.get(b"a").unwrap(), Some(Bytes::from_static(b"1")));
    tx.commit().unwrap();
}

#[test]
fn rollback_discards() {
    let (engine, _dir) = open_engine();
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"a", b"1").unwrap();
    tx.rollback().unwrap();
    assert_eq!(engine.get(b"a").unwrap(), None);
}

#[test]
fn concurrent_readers_with_writer() {
    let (engine, _dir) = open_engine();
    let engine = Arc::new(engine);

    let writer = {
        let engine = Arc::clone(&engine);
        thread::spawn(move || {
            for i in 0..500u32 {
                let key = format!("key{:08}", i);
                let mut tx = engine.begin(TxnOptions::default()).unwrap();
                tx.put(key.as_bytes(), b"written").unwrap();
                tx.commit().unwrap();
            }
        })
    };

    let reader = {
        let engine = Arc::clone(&engine);
        thread::spawn(move || {
            for _ in 0..1_000 {
                let _ = engine.get(b"key00000000");
            }
        })
    };

    writer.join().unwrap();
    reader.join().unwrap();
}

#[test]
fn concurrent_writers_stress() {
    let (engine, _dir) = open_engine();
    let engine = Arc::new(engine);

    const THREADS: usize = 8;
    const KEYS_PER_THREAD: usize = 200;

    let handles: Vec<_> = (0..THREADS)
        .map(|t| {
            let engine = Arc::clone(&engine);
            thread::spawn(move || {
                for i in 0..KEYS_PER_THREAD {
                    let key = format!("t{}k{}", t, i);
                    let value = format!("v{}", i);
                    let mut tx = engine.begin(TxnOptions::default()).unwrap();
                    tx.put(key.as_bytes(), value.as_bytes()).unwrap();
                    tx.commit().unwrap();
                }
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }

    for t in 0..THREADS {
        for i in 0..KEYS_PER_THREAD {
            let key = format!("t{}k{}", t, i);
            let expected = format!("v{}", i);
            assert_eq!(
                engine.get(key.as_bytes()).unwrap(),
                Some(Bytes::from(expected)),
                "missing key {}",
                key
            );
        }
    }
}

#[test]
fn check_integrity_passes_after_workload() {
    let (engine, _dir) = open_engine();

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    for i in 0..200u32 {
        let key = format!("k{:08}", i);
        tx.put(key.as_bytes(), format!("v{}", i).as_bytes())
            .unwrap();
    }
    tx.commit().unwrap();
    engine.check_integrity().unwrap();

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    for i in (0..200u32).step_by(3) {
        let key = format!("k{:08}", i);
        tx.delete(key.as_bytes()).unwrap();
    }
    tx.commit().unwrap();
    engine.check_integrity().unwrap();

    engine.sync().unwrap();
    engine.check_integrity().unwrap();
}

#[test]
fn stats_account_for_all_files() {
    let (engine, _dir) = open_engine();
    let before = engine.stats().unwrap();

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"a", &[0u8; 10_000]).unwrap();
    tx.commit().unwrap();

    let after = engine.stats().unwrap();
    assert!(after.disk_bytes >= before.disk_bytes);
    assert!(after.metrics.contains_key("storage_btree.retired_pages"));
    assert!(
        after
            .metrics
            .contains_key("storage_btree.cache_memory_bytes")
    );
}

#[test]
fn recover_after_meta_deleted() {
    let (engine, dir) = open_engine();

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"a", b"1").unwrap();
    tx.put(b"b", b"2").unwrap();
    tx.commit().unwrap();
    // Do NOT sync; the WAL still contains the committed records. Losing META
    // should be recoverable by replaying the WAL from scratch.
    drop(engine);

    std::fs::remove_file(dir.path().join("META")).unwrap();

    let engine = reopen_engine(&dir);
    assert_eq!(engine.get(b"a").unwrap(), Some(Bytes::from_static(b"1")));
    assert_eq!(engine.get(b"b").unwrap(), Some(Bytes::from_static(b"2")));
    engine.check_integrity().unwrap();
}

#[test]
fn scan_across_split_boundary() {
    let dir = tempfile::tempdir().unwrap();
    let engine = BtreeEngine::open(dir.path(), small_page_options()).unwrap();

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    for i in 0..500u32 {
        let key = format!("k{:08}", i);
        tx.put(key.as_bytes(), format!("v{}", i).as_bytes())
            .unwrap();
    }
    tx.commit().unwrap();
    engine.check_integrity().unwrap();

    let items: Vec<_> = engine
        .scan(None, None)
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(items.len(), 500);
    for (i, (key, value)) in items.iter().enumerate() {
        assert_eq!(key.as_ref(), format!("k{:08}", i).as_bytes());
        assert_eq!(value.as_ref(), format!("v{}", i).as_bytes());
    }
}

#[test]
fn delete_many_and_scan() {
    let dir = tempfile::tempdir().unwrap();
    let engine = BtreeEngine::open(dir.path(), small_page_options()).unwrap();

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    for i in 0..200u32 {
        let key = format!("k{:08}", i);
        tx.put(key.as_bytes(), format!("v{}", i).as_bytes())
            .unwrap();
    }
    tx.commit().unwrap();

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    for i in (0..200u32).step_by(2) {
        let key = format!("k{:08}", i);
        tx.delete(key.as_bytes()).unwrap();
    }
    tx.commit().unwrap();
    engine.check_integrity().unwrap();

    let keys: Vec<_> = engine
        .scan(None, None)
        .unwrap()
        .map(|r| r.unwrap().0)
        .collect();
    assert_eq!(keys.len(), 100);
    for (i, key) in keys.iter().enumerate() {
        let expected = format!("k{:08}", i * 2 + 1);
        assert_eq!(key.as_ref(), expected.as_bytes());
    }
}

#[test]
fn max_value_size_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let engine = BtreeEngine::open(
        dir.path(),
        BtreeOptions {
            page_size: 4096,
            max_inline_value_size: 1024,
            min_fill_percent: 50,
            cache_size: 0,
            max_value_size: 64,
            max_batch_ops: 10_000,
            ..Default::default()
        },
    )
    .unwrap();

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    assert!(tx.put(b"k", &[0u8; 65]).is_err());

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"k", &[0u8; 64]).unwrap();
    tx.commit().unwrap();
}

fn retired_pages(engine: &BtreeEngine) -> u64 {
    engine
        .stats()
        .unwrap()
        .metrics
        .get("storage_btree.retired_pages")
        .copied()
        .unwrap_or(0)
}

#[test]
fn active_snapshot_pins_old_pages() {
    let dir = tempfile::tempdir().unwrap();
    let engine = BtreeEngine::open(dir.path(), small_page_options()).unwrap();

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    for i in 0..200u32 {
        let key = format!("k{:08}", i);
        tx.put(key.as_bytes(), format!("v{}", i).as_bytes())
            .unwrap();
    }
    tx.commit().unwrap();

    // Pin the original snapshot with a read-only transaction.
    let reader = engine.begin(TxnOptions::read_only()).unwrap();

    // Mutate the tree so the original root is retired.
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    for i in 0..200u32 {
        let key = format!("k{:08}", i);
        tx.put(key.as_bytes(), format!("new{}", i).as_bytes())
            .unwrap();
    }
    tx.commit().unwrap();

    engine.compact().unwrap();
    engine.check_integrity().unwrap();

    // The reader must still see the original snapshot.
    for i in 0..200u32 {
        let key = format!("k{:08}", i);
        let expected = format!("v{}", i);
        assert_eq!(
            reader.get(key.as_bytes()).unwrap(),
            Some(Bytes::from(expected)),
            "reader lost pinned snapshot at key {}",
            key
        );
    }

    // Dropping the reader allows the pinned pages to be reclaimed.
    drop(reader);
    engine.compact().unwrap();
    engine.check_integrity().unwrap();
    assert_eq!(
        retired_pages(&engine),
        0,
        "reader dropped, all pages reclaimable"
    );
}

#[test]
fn value_log_dead_space_reclaimed() {
    let dir = tempfile::tempdir().unwrap();
    let engine = BtreeEngine::open(dir.path(), small_page_options()).unwrap();

    let large = vec![0xABu8; 8_192];
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"big", &large).unwrap();
    tx.commit().unwrap();
    engine.sync().unwrap();
    let value_log_before = std::fs::metadata(dir.path().join("values.log"))
        .unwrap()
        .len();

    // Overwrite with a tiny value; the old value-log record becomes dead.
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"big", b"small").unwrap();
    tx.commit().unwrap();
    engine.sync().unwrap();

    // Stop-the-world value-log GC reclaims the dead record.
    let _ = engine.compact_value_log().unwrap();
    engine.check_integrity_with_value_log().unwrap();

    assert_eq!(
        engine.get(b"big").unwrap(),
        Some(Bytes::from_static(b"small"))
    );

    let value_log_after = std::fs::metadata(dir.path().join("values.log"))
        .unwrap()
        .len();
    assert!(
        value_log_after < value_log_before,
        "value log did not shrink after GC: before={value_log_before}, after={value_log_after}"
    );
}

#[test]
fn file_size_stabilizes_after_updates() {
    let dir = tempfile::tempdir().unwrap();
    let engine = BtreeEngine::open(
        dir.path(),
        BtreeOptions {
            page_size: 512,
            max_inline_value_size: 64,
            min_fill_percent: 50,
            cache_size: 64 * 1024 * 1024,
            max_value_size: 16 * 1024 * 1024,
            max_batch_ops: 10_000,
            ..Default::default()
        },
    )
    .unwrap();

    // Establish a working set.
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    for i in 0..200u32 {
        let key = format!("k{:08}", i);
        tx.put(key.as_bytes(), format!("v{}", i).as_bytes())
            .unwrap();
    }
    tx.commit().unwrap();
    engine.sync().unwrap();
    let baseline = std::fs::metadata(dir.path().join("pages.dat"))
        .unwrap()
        .len();

    // Churn the same keyspace many times.
    for round in 0..10 {
        let mut tx = engine.begin(TxnOptions::default()).unwrap();
        for i in 0..200u32 {
            let key = format!("k{:08}", i);
            tx.put(key.as_bytes(), format!("r{}v{}", round, i).as_bytes())
                .unwrap();
        }
        tx.commit().unwrap();
        engine.sync().unwrap();
    }

    let final_size = std::fs::metadata(dir.path().join("pages.dat"))
        .unwrap()
        .len();
    assert!(
        final_size <= baseline.saturating_mul(2),
        "pages.dat grew from {} to {} despite compaction",
        baseline,
        final_size
    );
    engine.check_integrity().unwrap();
}

#[test]
fn concurrent_reader_writer_compact() {
    let dir = tempfile::tempdir().unwrap();
    let engine = BtreeEngine::open(dir.path(), small_page_options()).unwrap();

    let engine = Arc::new(engine);

    // Seed some data.
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    for i in 0..200u32 {
        let key = format!("k{:08}", i);
        tx.put(key.as_bytes(), format!("v{}", i).as_bytes())
            .unwrap();
    }
    tx.commit().unwrap();

    let reader = {
        let engine = Arc::clone(&engine);
        thread::spawn(move || {
            // Hold a snapshot across many writer/compactor rounds.
            let reader = engine.begin(TxnOptions::read_only()).unwrap();
            let mut last = Vec::new();
            for _ in 0..20 {
                let cursor = reader.scan(None, None).unwrap();
                let keys: Vec<_> = cursor.map(|r| r.unwrap().0).collect();
                if last.is_empty() {
                    last = keys;
                } else {
                    assert_eq!(last, keys, "reader snapshot changed");
                }
            }
        })
    };

    let writer = {
        let engine = Arc::clone(&engine);
        thread::spawn(move || {
            for round in 0..20 {
                let mut tx = engine.begin(TxnOptions::default()).unwrap();
                for i in 0..200u32 {
                    let key = format!("k{:08}", i);
                    tx.put(key.as_bytes(), format!("w{}v{}", round, i).as_bytes())
                        .unwrap();
                }
                tx.commit().unwrap();
                engine.sync().unwrap();
            }
        })
    };

    reader.join().unwrap();
    writer.join().unwrap();
    engine.check_integrity().unwrap();
    assert_eq!(retired_pages(&engine), 0);
}

#[test]
fn write_after_sync_survives_reopen() {
    // Regression test for the active-WAL-segment truncation bug: sync() must
    // not truncate the segment that the committer still has open.
    let dir = tempfile::tempdir().unwrap();
    let engine = BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap();

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"first", b"1").unwrap();
    tx.commit().unwrap();
    engine.sync().unwrap();

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"second", b"2").unwrap();
    tx.commit().unwrap();
    drop(engine);

    let engine = BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap();
    assert_eq!(
        engine.get(b"first").unwrap(),
        Some(Bytes::from_static(b"1"))
    );
    assert_eq!(
        engine.get(b"second").unwrap(),
        Some(Bytes::from_static(b"2"))
    );
}

#[test]
fn max_batch_ops_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let engine = BtreeEngine::open(
        dir.path(),
        BtreeOptions {
            page_size: 4096,
            max_inline_value_size: 1024,
            min_fill_percent: 50,
            cache_size: 0,
            max_value_size: 16 * 1024 * 1024,
            max_batch_ops: 3,
            ..Default::default()
        },
    )
    .unwrap();

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"a", b"1").unwrap();
    tx.put(b"b", b"2").unwrap();
    tx.put(b"c", b"3").unwrap();
    tx.put(b"d", b"4").unwrap();
    assert!(tx.commit().is_err());
}

// ---------------------------------------------------------------------------
// Engine-level deterministic fault-injection tests
// ---------------------------------------------------------------------------

/// Corrupt a data page that was flushed as part of a checkpoint.  The engine
/// must detect the checksum mismatch on the next open rather than returning
/// stale or garbage data.
#[test]
fn torn_page_detected_after_checkpoint() {
    let dir = tempfile::tempdir().unwrap();
    let engine = BtreeEngine::open(dir.path(), small_page_options()).unwrap();

    // Insert enough data to trigger at least one split (root stays page 1,
    // page 2 becomes a leaf).
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    for i in 0..20u32 {
        let key = format!("k{:08}", i);
        tx.put(key.as_bytes(), format!("v{}", i).as_bytes())
            .unwrap();
    }
    tx.commit().unwrap();

    // Sync flushes dirty pages and writes META; afterwards the leaf pages are
    // on disk and WAL before the checkpoint may be truncated.
    engine.sync().unwrap();
    drop(engine);

    // Simulate a torn write on page 2 by overwriting its body but leaving the
    // header/checksum region untouched so the checksum verification fails.
    const PAGE_SIZE: usize = 512;
    let page_path = dir.path().join("pages.dat");
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&page_path)
        .unwrap();
    let offset = 2 * PAGE_SIZE as u64;
    file.seek(SeekFrom::Start(offset + 64)).unwrap();
    file.write_all(&[0u8; PAGE_SIZE - 64]).unwrap();
    file.flush().unwrap();
    drop(file);

    // Reopening must detect the corruption.
    let result = BtreeEngine::open(dir.path(), small_page_options());
    assert!(
        result.is_err(),
        "opening with a torn page should fail with a corruption error"
    );
}

/// A partially-written or corrupt primary META file must be recoverable from
/// the `META.bak` backup written by the previous checkpoint.  This test also
/// verifies that the backup can be stale: data committed after the backup was
/// taken is replayed from the WAL.
#[test]
fn partial_meta_recovered_from_backup() {
    let dir = tempfile::tempdir().unwrap();
    let opts = small_page_options();

    // First checkpoint: writes META v1 and META.bak v1.
    let engine = BtreeEngine::open(dir.path(), opts.clone()).unwrap();
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"first", b"1").unwrap();
    tx.commit().unwrap();
    engine.sync().unwrap();
    drop(engine);

    // Second checkpoint: overwrites META with v2; backup now holds v1.
    let engine = BtreeEngine::open(dir.path(), opts.clone()).unwrap();
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"second", b"2").unwrap();
    tx.commit().unwrap();
    engine.sync().unwrap();
    drop(engine);

    // Corrupt the primary META file.
    let meta_path = dir.path().join("META");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(&meta_path)
        .unwrap();
    file.write_all(&[0u8; 32]).unwrap();
    file.flush().unwrap();
    drop(file);

    // Reopen must fall back to META.bak and replay the WAL to recover both
    // keys.
    let engine = BtreeEngine::open(dir.path(), opts.clone()).unwrap();
    assert_eq!(
        engine.get(b"first").unwrap(),
        Some(Bytes::from_static(b"1"))
    );
    assert_eq!(
        engine.get(b"second").unwrap(),
        Some(Bytes::from_static(b"2"))
    );
    engine.check_integrity().unwrap();
}

/// Recovery rebuilds the in-memory value-log reference counts by scanning every
/// live leaf cell.  After reopening, an old large value can be safely reclaimed
/// by value-log compaction because the refcounts correctly reflect the current
/// tree contents.
#[test]
fn value_log_refcounts_rebuilt_after_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let opts = BtreeOptions {
        page_size: 512,
        max_inline_value_size: 64,
        min_fill_percent: 50,
        min_cells: Some(1),
        cache_size: 0,
        max_value_size: 16 * 1024 * 1024,
        max_batch_ops: 10_000,
        ..Default::default()
    };

    let large1: Vec<u8> = (0..200).map(|i| (i % 256) as u8).collect();
    let large2: Vec<u8> = (0..200).map(|i| ((i + 7) % 256) as u8).collect();

    let engine = BtreeEngine::open(dir.path(), opts.clone()).unwrap();
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"k", &large1).unwrap();
    tx.commit().unwrap();
    engine.sync().unwrap();
    drop(engine);

    // Reopen: recovery scans leaves and rebuilds ref counts from scratch.
    let engine = BtreeEngine::open(dir.path(), opts.clone()).unwrap();
    assert_eq!(engine.get(b"k").unwrap(), Some(Bytes::from(large1.clone())));

    // Overwrite with a new large value, releasing the old reference.
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"k", &large2).unwrap();
    tx.commit().unwrap();
    engine.sync().unwrap();

    let value_log_before = std::fs::metadata(dir.path().join("values.log"))
        .unwrap()
        .len();

    // Compact the value log; the old value should be reclaimable because the
    // rebuilt reference counts know it is no longer reachable.
    engine.compact_value_log().unwrap();

    let value_log_after = std::fs::metadata(dir.path().join("values.log"))
        .unwrap()
        .len();
    assert!(
        value_log_after < value_log_before,
        "value log did not shrink after replacing a large value: before={}, after={}",
        value_log_before,
        value_log_after
    );

    assert_eq!(engine.get(b"k").unwrap(), Some(Bytes::from(large2)));
    engine.check_integrity_with_value_log().unwrap();
}
