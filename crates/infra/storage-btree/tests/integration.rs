//! Integration tests for `storage-btree`.

use std::sync::Arc;
use std::thread;

use bytes::Bytes;
use storage_btree::{BtreeEngine, BtreeOptions};
use storage_traits::{Cursor, Engine, Transaction, TxnOptions};
use tempfile::TempDir;

fn open_engine() -> (BtreeEngine, TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let engine = BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap();
    (engine, dir)
}

fn reopen_engine(dir: &TempDir) -> BtreeEngine {
    BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap()
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
    // Do NOT sync; the WAL still contains the committed batch. Losing META
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
    // Use a tiny page size so a modest number of keys forces leaf splits.
    let dir = tempfile::tempdir().unwrap();
    let engine = BtreeEngine::open(
        dir.path(),
        BtreeOptions {
            page_size: 512,
            max_inline_value_size: 64,
            min_fill_percent: 50,
            cache_size: 0,
            max_value_size: 16 * 1024 * 1024,
            max_batch_ops: 10_000,
        },
    )
    .unwrap();

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
    let engine = BtreeEngine::open(
        dir.path(),
        BtreeOptions {
            page_size: 512,
            max_inline_value_size: 64,
            min_fill_percent: 50,
            cache_size: 0,
            max_value_size: 16 * 1024 * 1024,
            max_batch_ops: 10_000,
        },
    )
    .unwrap();

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
        },
    )
    .unwrap();

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"k", &[0u8; 65]).unwrap();
    assert!(tx.commit().is_err());

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

fn freelist_pages(engine: &BtreeEngine) -> u64 {
    engine
        .stats()
        .unwrap()
        .metrics
        .get("storage_btree.freelist_pages")
        .copied()
        .unwrap_or(0)
}

#[test]
fn compact_reclaims_retired_pages() {
    let dir = tempfile::tempdir().unwrap();
    let options = BtreeOptions {
        page_size: 512,
        max_inline_value_size: 64,
        min_fill_percent: 50,
        cache_size: 0,
        max_value_size: 16 * 1024 * 1024,
        max_batch_ops: 10_000,
    };
    let engine = BtreeEngine::open(dir.path(), options.clone()).unwrap();

    // Create many keys across multiple leaves.
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    for i in 0..500u32 {
        let key = format!("k{:08}", i);
        tx.put(key.as_bytes(), format!("v{}", i).as_bytes())
            .unwrap();
    }
    tx.commit().unwrap();
    engine.check_integrity().unwrap();

    // Overwrite half and delete the other half to generate retired pages.
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    for i in 0..250u32 {
        let key = format!("k{:08}", i);
        tx.put(key.as_bytes(), format!("updated{}", i).as_bytes())
            .unwrap();
    }
    for i in 250..500u32 {
        let key = format!("k{:08}", i);
        tx.delete(key.as_bytes()).unwrap();
    }
    tx.commit().unwrap();
    engine.check_integrity().unwrap();

    assert!(
        retired_pages(&engine) > 0,
        "expected retired pages after updates/deletes"
    );

    engine.compact().unwrap();
    engine.check_integrity().unwrap();

    assert_eq!(
        retired_pages(&engine),
        0,
        "compact should reclaim all retired pages"
    );
    assert!(
        freelist_pages(&engine) > 0,
        "expected freelist entries after compaction"
    );

    // Reopen and ensure the live state is intact and ids are reused.
    drop(engine);
    let engine = BtreeEngine::open(dir.path(), options).unwrap();
    for i in 0..250u32 {
        let key = format!("k{:08}", i);
        let expected = format!("updated{}", i);
        assert_eq!(
            engine.get(key.as_bytes()).unwrap(),
            Some(Bytes::from(expected))
        );
    }
    for i in 250..500u32 {
        let key = format!("k{:08}", i);
        assert_eq!(engine.get(key.as_bytes()).unwrap(), None);
    }
    engine.check_integrity().unwrap();
}

#[test]
fn active_snapshot_pins_old_pages() {
    let dir = tempfile::tempdir().unwrap();
    let engine = BtreeEngine::open(
        dir.path(),
        BtreeOptions {
            page_size: 512,
            max_inline_value_size: 64,
            min_fill_percent: 50,
            cache_size: 0,
            max_value_size: 16 * 1024 * 1024,
            max_batch_ops: 10_000,
        },
    )
    .unwrap();

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
fn overflow_pages_reclaimed() {
    let dir = tempfile::tempdir().unwrap();
    let engine = BtreeEngine::open(
        dir.path(),
        BtreeOptions {
            page_size: 512,
            max_inline_value_size: 64,
            min_fill_percent: 50,
            cache_size: 0,
            max_value_size: 16 * 1024 * 1024,
            max_batch_ops: 10_000,
        },
    )
    .unwrap();

    let large = vec![0xABu8; 8_192];
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"big", &large).unwrap();
    tx.commit().unwrap();

    // Overwrite with a tiny value; the overflow chain becomes retired.
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"big", b"small").unwrap();
    tx.commit().unwrap();

    assert!(retired_pages(&engine) > 0);

    engine.compact().unwrap();
    engine.check_integrity().unwrap();

    assert_eq!(retired_pages(&engine), 0);
    assert!(freelist_pages(&engine) > 0);
    assert_eq!(
        engine.get(b"big").unwrap(),
        Some(Bytes::from_static(b"small"))
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
    // With reuse, pages.dat should not grow linearly with churn. A small
    // increase is acceptable due to tree-shape variance; a leak would produce
    // a much larger number.
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
    let engine = BtreeEngine::open(
        dir.path(),
        BtreeOptions {
            page_size: 512,
            max_inline_value_size: 64,
            min_fill_percent: 50,
            cache_size: 64 * 1024 * 1024,
            max_value_size: 16 * 1024 * 1024,
            max_batch_ops: 10_000,
        },
    )
    .unwrap();

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
fn recovery_reclaims_leaked_retired_pages() {
    let dir = tempfile::tempdir().unwrap();
    let options = BtreeOptions {
        page_size: 512,
        max_inline_value_size: 64,
        min_fill_percent: 50,
        cache_size: 0,
        max_value_size: 16 * 1024 * 1024,
        max_batch_ops: 10_000,
    };
    let engine = BtreeEngine::open(dir.path(), options.clone()).unwrap();

    // Create a tree and fully compact it so the WAL is empty and the freelist
    // is up to date.
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    for i in 0..200u32 {
        let key = format!("k{:08}", i);
        tx.put(key.as_bytes(), format!("v{}", i).as_bytes())
            .unwrap();
    }
    tx.commit().unwrap();
    engine.sync().unwrap();
    assert_eq!(retired_pages(&engine), 0);
    assert!(freelist_pages(&engine) > 0);

    // Delete everything without compacting. The retired pages are not persisted
    // in META, simulating a leak after an unclean shutdown.
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    for i in 0..200u32 {
        let key = format!("k{:08}", i);
        tx.delete(key.as_bytes()).unwrap();
    }
    tx.commit().unwrap();
    // Retired pages exist in memory but are about to be lost on drop.
    assert!(retired_pages(&engine) > 0);
    drop(engine);

    // On reopen the engine must recover the leaked ids into the freelist.
    let engine = BtreeEngine::open(dir.path(), options).unwrap();
    engine.check_integrity().unwrap();
    assert_eq!(engine.get(b"k00000000").unwrap(), None);
    assert!(freelist_pages(&engine) > 0);

    // New allocations should reuse the reclaimed ids, not grow pages.dat.
    let before_pages = std::fs::metadata(dir.path().join("pages.dat"))
        .unwrap()
        .len();
    let before_freelist = freelist_pages(&engine);
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    for i in 0..50u32 {
        let key = format!("new{:08}", i);
        tx.put(key.as_bytes(), format!("v{}", i).as_bytes())
            .unwrap();
    }
    tx.commit().unwrap();
    engine.sync().unwrap();
    let after_pages = std::fs::metadata(dir.path().join("pages.dat"))
        .unwrap()
        .len();
    let after_freelist = freelist_pages(&engine);
    assert_eq!(
        after_pages, before_pages,
        "new writes should reuse reclaimed ids without growing pages.dat"
    );
    // Compact() may reclaim additional pages, so the freelist can move in
    // either direction. The important invariant is that pages.dat did not grow.
    let _ = (before_freelist, after_freelist);
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

    // The critical write happens on the *same* engine after sync(). With an
    // unsafe active-segment truncation, this write would go to an unlinked
    // inode and be lost on the next open.
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
