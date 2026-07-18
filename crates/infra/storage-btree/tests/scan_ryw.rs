//! Tests for scan read-your-writes.
//!
//! These tests verify that a transaction's range scans see the same
//! uncommitted data as its point reads.

use bytes::Bytes;
use storage_btree::{BtreeEngine, BtreeOptions};
use storage_traits::{Engine, Transaction};

fn make_engine() -> (BtreeEngine, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let engine = BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap();
    (engine, dir)
}

#[test]
fn scan_sees_uncommitted_put() {
    let (engine, _dir) = make_engine();

    // Pre-populate with committed data.
    let mut txn = engine
        .begin(storage_traits::TxnOptions {
            read_only: false,
            isolation: storage_traits::IsolationLevel::Snapshot,
        })
        .unwrap();
    txn.put(b"a", b"committed-a").unwrap();
    txn.put(b"c", b"committed-c").unwrap();
    txn.commit().unwrap();

    // New transaction overwrites and inserts within the range.
    let mut txn = engine
        .begin(storage_traits::TxnOptions {
            read_only: false,
            isolation: storage_traits::IsolationLevel::Snapshot,
        })
        .unwrap();
    txn.put(b"a", b"uncommitted-a").unwrap();
    txn.put(b"b", b"uncommitted-b").unwrap();

    let cursor = txn.scan(Some(b"a"), Some(b"d")).unwrap();
    let mut got: Vec<(Bytes, Bytes)> = Vec::new();
    for item in cursor {
        got.push(item.unwrap());
    }

    assert_eq!(
        got,
        vec![
            (
                Bytes::from_static(b"a"),
                Bytes::from_static(b"uncommitted-a")
            ),
            (
                Bytes::from_static(b"b"),
                Bytes::from_static(b"uncommitted-b")
            ),
            (Bytes::from_static(b"c"), Bytes::from_static(b"committed-c")),
        ]
    );
}

#[test]
fn scan_hides_uncommitted_delete() {
    let (engine, _dir) = make_engine();

    let mut txn = engine
        .begin(storage_traits::TxnOptions {
            read_only: false,
            isolation: storage_traits::IsolationLevel::Snapshot,
        })
        .unwrap();
    txn.put(b"a", b"a").unwrap();
    txn.put(b"b", b"b").unwrap();
    txn.put(b"c", b"c").unwrap();
    txn.commit().unwrap();

    let mut txn = engine
        .begin(storage_traits::TxnOptions {
            read_only: false,
            isolation: storage_traits::IsolationLevel::Snapshot,
        })
        .unwrap();
    txn.delete(b"b").unwrap();

    let cursor = txn.scan(None, None).unwrap();
    let keys: Vec<Bytes> = cursor.map(|r| r.unwrap().0).collect();

    assert_eq!(
        keys,
        vec![Bytes::from_static(b"a"), Bytes::from_static(b"c")]
    );
}

#[test]
fn scan_uses_overwrite_value() {
    let (engine, _dir) = make_engine();

    let mut txn = engine
        .begin(storage_traits::TxnOptions {
            read_only: false,
            isolation: storage_traits::IsolationLevel::Snapshot,
        })
        .unwrap();
    txn.put(b"a", b"old").unwrap();
    txn.commit().unwrap();

    let mut txn = engine
        .begin(storage_traits::TxnOptions {
            read_only: false,
            isolation: storage_traits::IsolationLevel::Snapshot,
        })
        .unwrap();
    txn.put(b"a", b"new").unwrap();

    let mut cursor = txn.scan(None, None).unwrap();
    let item = cursor.next().unwrap().unwrap();
    assert_eq!(item.0, Bytes::from_static(b"a"));
    assert_eq!(item.1, Bytes::from_static(b"new"));
}

#[test]
fn scan_ryw_with_splits() {
    let (engine, _dir) = make_engine();

    // Insert enough keys to trigger splits.
    let mut txn = engine
        .begin(storage_traits::TxnOptions {
            read_only: false,
            isolation: storage_traits::IsolationLevel::Snapshot,
        })
        .unwrap();
    for i in 0u64..100 {
        let key = format!("{:04x}", i);
        txn.put(key.as_bytes(), key.as_bytes()).unwrap();
    }

    // Insert a new key that may land in a different leaf after splits.
    txn.put(b"ffff", b"uncommitted").unwrap();

    let cursor = txn.scan(Some(b"ff"), None).unwrap();
    let items: Vec<(Bytes, Bytes)> = cursor.map(|r| r.unwrap()).collect();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].0, Bytes::from_static(b"ffff"));
    assert_eq!(items[0].1, Bytes::from_static(b"uncommitted"));
}

#[test]
fn rollback_clears_write_set_from_scan() {
    let (engine, _dir) = make_engine();

    let mut txn = engine
        .begin(storage_traits::TxnOptions {
            read_only: false,
            isolation: storage_traits::IsolationLevel::Snapshot,
        })
        .unwrap();
    txn.put(b"a", b"a").unwrap();
    txn.commit().unwrap();

    let mut txn = engine
        .begin(storage_traits::TxnOptions {
            read_only: false,
            isolation: storage_traits::IsolationLevel::Snapshot,
        })
        .unwrap();
    txn.put(b"b", b"b").unwrap();
    txn.rollback().unwrap();

    // A new transaction must not see the rolled-back write.
    let txn = engine
        .begin(storage_traits::TxnOptions {
            read_only: true,
            isolation: storage_traits::IsolationLevel::Snapshot,
        })
        .unwrap();
    let cursor = txn.scan(None, None).unwrap();
    let keys: Vec<Bytes> = cursor.map(|r| r.unwrap().0).collect();
    assert_eq!(keys, vec![Bytes::from_static(b"a")]);
}

#[test]
fn read_only_txn_scan_unchanged() {
    let (engine, _dir) = make_engine();

    let mut txn = engine
        .begin(storage_traits::TxnOptions {
            read_only: false,
            isolation: storage_traits::IsolationLevel::Snapshot,
        })
        .unwrap();
    txn.put(b"a", b"a").unwrap();
    txn.commit().unwrap();

    let txn = engine
        .begin(storage_traits::TxnOptions {
            read_only: true,
            isolation: storage_traits::IsolationLevel::Snapshot,
        })
        .unwrap();
    let cursor = txn.scan(None, None).unwrap();
    let keys: Vec<Bytes> = cursor.map(|r| r.unwrap().0).collect();
    assert_eq!(keys, vec![Bytes::from_static(b"a")]);
}
