//! Tests for operational APIs: backup, metrics, and file shrink.

use bytes::Bytes;
use storage_btree::{BtreeEngine, BtreeOptions};
use storage_traits::{Engine, Transaction};

fn make_engine() -> (BtreeEngine, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let engine = BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap();
    (engine, dir)
}

#[test]
fn backup_produces_openable_copy() {
    let (engine, dir) = make_engine();

    let mut txn = engine
        .begin(storage_traits::TxnOptions {
            read_only: false,
            isolation: storage_traits::IsolationLevel::Snapshot,
        })
        .unwrap();
    txn.put(b"hello", b"world").unwrap();
    txn.put(b"foo", b"bar").unwrap();
    txn.commit().unwrap();

    let backup_dir = dir.path().join("backup");
    engine.backup(&backup_dir).unwrap();

    let restored = BtreeEngine::open(&backup_dir, BtreeOptions::default()).unwrap();
    assert_eq!(
        restored.get(b"hello").unwrap(),
        Some(Bytes::from_static(b"world"))
    );
    assert_eq!(
        restored.get(b"foo").unwrap(),
        Some(Bytes::from_static(b"bar"))
    );
    restored.check_integrity().unwrap();
}

#[test]
fn metrics_track_operations() {
    let (engine, _dir) = make_engine();

    let before = engine.metrics();

    let mut txn = engine
        .begin(storage_traits::TxnOptions {
            read_only: false,
            isolation: storage_traits::IsolationLevel::Snapshot,
        })
        .unwrap();
    txn.put(b"a", b"1").unwrap();
    txn.put(b"b", b"2").unwrap();
    txn.get(b"a").unwrap();
    txn.commit().unwrap();

    let after = engine.metrics();
    assert!(after.puts >= before.puts + 2, "puts should increase");
    assert!(after.gets > before.gets, "gets should increase");
    assert!(
        after.txns_begun > before.txns_begun,
        "txns_begun should increase"
    );
    assert!(
        after.txns_committed > before.txns_committed,
        "txns_committed should increase"
    );
}

#[test]
fn metrics_track_cache_hits_and_misses() {
    let (engine, _dir) = make_engine();

    let mut txn = engine
        .begin(storage_traits::TxnOptions {
            read_only: false,
            isolation: storage_traits::IsolationLevel::Snapshot,
        })
        .unwrap();
    for i in 0u64..10 {
        let key = format!("{:08x}", i);
        txn.put(key.as_bytes(), key.as_bytes()).unwrap();
    }
    txn.commit().unwrap();

    // Reading back should produce hits (some may be misses as pages are
    // installed).
    let before = engine.metrics();
    for i in 0u64..10 {
        let key = format!("{:08x}", i);
        engine.get(key.as_bytes()).unwrap();
    }
    let after = engine.metrics();
    assert!(
        after.cache_hits + after.cache_misses >= before.cache_hits + before.cache_misses + 10,
        "cache access counters should increase"
    );
}

#[test]
fn stats_include_btree_counters() {
    let (engine, _dir) = make_engine();

    let mut txn = engine
        .begin(storage_traits::TxnOptions {
            read_only: false,
            isolation: storage_traits::IsolationLevel::Snapshot,
        })
        .unwrap();
    txn.put(b"k", b"v").unwrap();
    txn.commit().unwrap();

    let stats = engine.stats().unwrap();
    assert!(stats.metrics.contains_key("storage_btree.puts"));
    assert!(stats.metrics.contains_key("storage_btree.cache_hits"));
}

#[test]
fn shrink_reduces_tail_free_space() {
    let (engine, dir) = make_engine();

    // Write a large number of keys to grow the page file.
    let mut txn = engine
        .begin(storage_traits::TxnOptions {
            read_only: false,
            isolation: storage_traits::IsolationLevel::Snapshot,
        })
        .unwrap();
    for i in 0u64..200 {
        let key = format!("{:08x}", i);
        txn.put(key.as_bytes(), key.as_bytes()).unwrap();
    }
    txn.commit().unwrap();
    engine.sync().unwrap();

    let size_before = std::fs::metadata(dir.path().join("pages.dat"))
        .unwrap()
        .len();

    // Delete most keys, then compact and shrink.
    let mut txn = engine
        .begin(storage_traits::TxnOptions {
            read_only: false,
            isolation: storage_traits::IsolationLevel::Snapshot,
        })
        .unwrap();
    for i in 100u64..200 {
        let key = format!("{:08x}", i);
        txn.delete(key.as_bytes()).unwrap();
    }
    txn.commit().unwrap();
    engine.shrink_pages_file().unwrap();

    let size_after = std::fs::metadata(dir.path().join("pages.dat"))
        .unwrap()
        .len();
    assert!(
        size_after <= size_before,
        "shrink should not grow the file: before={size_before}, after={size_after}"
    );

    // Remaining keys are still readable.
    for i in 0u64..100 {
        let key = format!("{:08x}", i);
        assert_eq!(
            engine.get(key.as_bytes()).unwrap(),
            Some(Bytes::from(key.clone())),
            "key {key} missing after shrink"
        );
    }

    // Deleted keys are gone.
    for i in 100u64..200 {
        let key = format!("{:08x}", i);
        assert!(
            engine.get(key.as_bytes()).unwrap().is_none(),
            "key {key} still present"
        );
    }

    engine.check_integrity().unwrap();
}

#[test]
fn backup_roundtrip_with_large_values() {
    let (engine, dir) = make_engine();

    let value = vec![b'x'; 4096];
    let mut txn = engine
        .begin(storage_traits::TxnOptions {
            read_only: false,
            isolation: storage_traits::IsolationLevel::Snapshot,
        })
        .unwrap();
    txn.put(b"big", &value).unwrap();
    txn.commit().unwrap();

    let backup_dir = dir.path().join("backup");
    engine.backup(&backup_dir).unwrap();

    let restored = BtreeEngine::open(&backup_dir, BtreeOptions::default()).unwrap();
    assert_eq!(restored.get(b"big").unwrap(), Some(Bytes::from(value)));
}
