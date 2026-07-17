//! Transaction + column-family integration tests.
//!
//! These verify that transactions capture per-column-family snapshots so that
//! reads and scans from a CF are isolated from concurrent writes to the same CF.

use storage_kv::{LsmEngine, LsmOptions};
use storage_traits::{Cursor, Engine, Transaction};
use tempfile::TempDir;

fn opts() -> LsmOptions {
    LsmOptions {
        write_buffer_size: 128,
        max_write_buffer_number: 2,
        ..Default::default()
    }
}

#[test]
fn cf_txn_point_read_snapshot_isolation() {
    let dir = TempDir::new().unwrap();
    let engine = LsmEngine::open(dir.path(), opts()).unwrap();
    let cf = engine.create_column_family("cf1", opts()).unwrap();

    engine.put_cf(&cf, b"key", b"initial").unwrap();

    let txn = engine.begin(Default::default()).unwrap();
    assert_eq!(
        txn.get_cf(&cf, b"key").unwrap(),
        Some(bytes::Bytes::from_static(b"initial"))
    );

    engine.put_cf(&cf, b"key", b"updated").unwrap();

    // The existing transaction must still see the value at the time it started.
    assert_eq!(
        txn.get_cf(&cf, b"key").unwrap(),
        Some(bytes::Bytes::from_static(b"initial"))
    );

    drop(txn);

    // A new transaction sees the updated value.
    let txn2 = engine.begin(Default::default()).unwrap();
    assert_eq!(
        txn2.get_cf(&cf, b"key").unwrap(),
        Some(bytes::Bytes::from_static(b"updated"))
    );
}

#[test]
fn cf_txn_read_your_writes() {
    let dir = TempDir::new().unwrap();
    let engine = LsmEngine::open(dir.path(), opts()).unwrap();
    let cf = engine.create_column_family("cf1", opts()).unwrap();

    engine.put_cf(&cf, b"key", b"base").unwrap();

    let mut txn = engine.begin(Default::default()).unwrap();
    txn.put_cf(&cf, b"key", b"written-in-txn").unwrap();

    // The transaction must see its own uncommitted write.
    assert_eq!(
        txn.get_cf(&cf, b"key").unwrap(),
        Some(bytes::Bytes::from_static(b"written-in-txn"))
    );

    // A concurrent transaction started before the write sees the old value.
    let txn2 = engine.begin(Default::default()).unwrap();
    assert_eq!(
        txn2.get_cf(&cf, b"key").unwrap(),
        Some(bytes::Bytes::from_static(b"base"))
    );

    // Rolling back the transaction leaves the original value in place.
    txn.rollback().unwrap();
    assert_eq!(
        engine.get_cf(&cf, b"key").unwrap(),
        Some(bytes::Bytes::from_static(b"base"))
    );
}

#[test]
fn cf_txn_scan_snapshot_isolation() {
    let dir = TempDir::new().unwrap();
    let engine = LsmEngine::open(dir.path(), opts()).unwrap();
    let cf = engine.create_column_family("cf1", opts()).unwrap();

    for i in 0..10 {
        engine
            .put_cf(&cf, format!("k{}", i).as_bytes(), b"base")
            .unwrap();
    }

    let txn = engine.begin(Default::default()).unwrap();

    // Concurrent modifications.
    engine.put_cf(&cf, b"k5", b"updated").unwrap();
    engine.delete_cf(&cf, b"k3").unwrap();

    let mut cursor = txn.scan_cf(&cf, Some(b"k0"), Some(b"k:")).unwrap();
    let mut rows: Vec<(String, bytes::Bytes)> = Vec::new();
    while let Some(Ok((k, v))) = cursor.next() {
        rows.push((String::from_utf8(k.to_vec()).unwrap(), v));
    }

    // Snapshot must not see the concurrent update or deletion.
    let k3 = rows.iter().find(|(k, _)| k == "k3");
    assert!(k3.is_some(), "k3 should still be visible in snapshot");

    let k5 = rows.iter().find(|(k, _)| k == "k5");
    assert!(k5.is_some(), "k5 should still be visible in snapshot");
    assert_eq!(k5.unwrap().1, bytes::Bytes::from_static(b"base"));

    // A new transaction sees the modifications.
    let txn2 = engine.begin(Default::default()).unwrap();
    let mut cursor2 = txn2.scan_cf(&cf, Some(b"k0"), Some(b"k:")).unwrap();
    let rows2: Vec<(String, bytes::Bytes)> = std::iter::from_fn(|| cursor2.next())
        .filter_map(|r| r.ok())
        .map(|(k, v)| (String::from_utf8(k.to_vec()).unwrap(), v))
        .collect();
    assert!(rows2.iter().find(|(k, _)| k == "k3").is_none());
    assert_eq!(
        rows2.iter().find(|(k, _)| k == "k5").unwrap().1,
        bytes::Bytes::from_static(b"updated")
    );
}

#[test]
fn cf_txn_scan_read_your_writes_put() {
    let dir = TempDir::new().unwrap();
    let engine = LsmEngine::open(dir.path(), opts()).unwrap();
    let cf = engine.create_column_family("cf1", opts()).unwrap();

    engine.put_cf(&cf, b"a", b"base-a").unwrap();
    engine.put_cf(&cf, b"c", b"base-c").unwrap();

    let mut txn = engine.begin(Default::default()).unwrap();
    txn.put_cf(&cf, b"b", b"txn-b").unwrap();

    let rows: Vec<(bytes::Bytes, bytes::Bytes)> = txn
        .scan_cf(&cf, Some(b"a"), Some(b"d"))
        .unwrap()
        .next_batch(100)
        .unwrap();

    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].0, bytes::Bytes::from_static(b"a"));
    assert_eq!(rows[0].1, bytes::Bytes::from_static(b"base-a"));
    assert_eq!(rows[1].0, bytes::Bytes::from_static(b"b"));
    assert_eq!(rows[1].1, bytes::Bytes::from_static(b"txn-b"));
    assert_eq!(rows[2].0, bytes::Bytes::from_static(b"c"));
    assert_eq!(rows[2].1, bytes::Bytes::from_static(b"base-c"));
}

#[test]
fn cf_txn_scan_read_your_writes_delete() {
    let dir = TempDir::new().unwrap();
    let engine = LsmEngine::open(dir.path(), opts()).unwrap();
    let cf = engine.create_column_family("cf1", opts()).unwrap();

    engine.put_cf(&cf, b"a", b"base-a").unwrap();
    engine.put_cf(&cf, b"b", b"base-b").unwrap();
    engine.put_cf(&cf, b"c", b"base-c").unwrap();

    let mut txn = engine.begin(Default::default()).unwrap();
    txn.delete_cf(&cf, b"b").unwrap();

    let rows: Vec<(bytes::Bytes, bytes::Bytes)> = txn
        .scan_cf(&cf, Some(b"a"), Some(b"d"))
        .unwrap()
        .next_batch(100)
        .unwrap();

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].0, bytes::Bytes::from_static(b"a"));
    assert_eq!(rows[1].0, bytes::Bytes::from_static(b"c"));
}

#[test]
fn cf_txn_scan_read_your_writes_range_delete() {
    let dir = TempDir::new().unwrap();
    let engine = LsmEngine::open(dir.path(), opts()).unwrap();
    let cf = engine.create_column_family("cf1", opts()).unwrap();

    for key in [b"a", b"b", b"c", b"d", b"e"] {
        engine.put_cf(&cf, key, b"base").unwrap();
    }

    let mut txn = engine.begin(Default::default()).unwrap();
    txn.delete_range_cf(&cf, b"b", b"d").unwrap();

    let rows: Vec<bytes::Bytes> = txn
        .scan_cf(&cf, Some(b"a"), Some(b"f"))
        .unwrap()
        .next_batch(100)
        .unwrap()
        .into_iter()
        .map(|(k, _)| k)
        .collect();

    assert_eq!(
        rows,
        vec![
            bytes::Bytes::from_static(b"a"),
            bytes::Bytes::from_static(b"d"),
            bytes::Bytes::from_static(b"e"),
        ]
    );
}

#[test]
fn cf_txn_scan_buffered_put_overrides_snapshot() {
    let dir = TempDir::new().unwrap();
    let engine = LsmEngine::open(dir.path(), opts()).unwrap();
    let cf = engine.create_column_family("cf1", opts()).unwrap();

    engine.put_cf(&cf, b"a", b"base").unwrap();

    let mut txn = engine.begin(Default::default()).unwrap();
    txn.put_cf(&cf, b"a", b"overridden").unwrap();

    let rows: Vec<(bytes::Bytes, bytes::Bytes)> = txn
        .scan_cf(&cf, None, None)
        .unwrap()
        .next_batch(100)
        .unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1, bytes::Bytes::from_static(b"overridden"));
}

#[test]
fn cf_txn_scan_interleaved_keys() {
    let dir = TempDir::new().unwrap();
    let engine = LsmEngine::open(dir.path(), opts()).unwrap();
    let cf = engine.create_column_family("cf1", opts()).unwrap();

    // Snapshot keys: a, c, e
    for key in [b"a", b"c", b"e"] {
        engine.put_cf(&cf, key, b"base").unwrap();
    }

    let mut txn = engine.begin(Default::default()).unwrap();
    // Buffered keys: b, d
    txn.put_cf(&cf, b"b", b"txn").unwrap();
    txn.put_cf(&cf, b"d", b"txn").unwrap();

    let rows: Vec<bytes::Bytes> = txn
        .scan_cf(&cf, None, None)
        .unwrap()
        .next_batch(100)
        .unwrap()
        .into_iter()
        .map(|(k, _)| k)
        .collect();

    assert_eq!(
        rows,
        vec![
            bytes::Bytes::from_static(b"a"),
            bytes::Bytes::from_static(b"b"),
            bytes::Bytes::from_static(b"c"),
            bytes::Bytes::from_static(b"d"),
            bytes::Bytes::from_static(b"e"),
        ]
    );
}

#[test]
fn cf_txn_scan_delete_then_put_same_key() {
    let dir = TempDir::new().unwrap();
    let engine = LsmEngine::open(dir.path(), opts()).unwrap();
    let cf = engine.create_column_family("cf1", opts()).unwrap();

    engine.put_cf(&cf, b"a", b"base").unwrap();

    let mut txn = engine.begin(Default::default()).unwrap();
    txn.delete_cf(&cf, b"a").unwrap();
    txn.put_cf(&cf, b"a", b"restored").unwrap();

    let rows: Vec<(bytes::Bytes, bytes::Bytes)> = txn
        .scan_cf(&cf, None, None)
        .unwrap()
        .next_batch(100)
        .unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1, bytes::Bytes::from_static(b"restored"));
}

#[test]
fn cf_txn_scan_range_delete_then_put_same_key() {
    let dir = TempDir::new().unwrap();
    let engine = LsmEngine::open(dir.path(), opts()).unwrap();
    let cf = engine.create_column_family("cf1", opts()).unwrap();

    engine.put_cf(&cf, b"b", b"base").unwrap();

    let mut txn = engine.begin(Default::default()).unwrap();
    txn.delete_range_cf(&cf, b"a", b"z").unwrap();
    txn.put_cf(&cf, b"b", b"after-tombstone").unwrap();

    let rows: Vec<(bytes::Bytes, bytes::Bytes)> = txn
        .scan_cf(&cf, None, None)
        .unwrap()
        .next_batch(100)
        .unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, bytes::Bytes::from_static(b"b"));
    assert_eq!(rows[0].1, bytes::Bytes::from_static(b"after-tombstone"));
}

#[test]
fn cf_txn_scan_seek_with_buffered_writes() {
    let dir = TempDir::new().unwrap();
    let engine = LsmEngine::open(dir.path(), opts()).unwrap();
    let cf = engine.create_column_family("cf1", opts()).unwrap();

    for key in [b"a", b"c", b"e"] {
        engine.put_cf(&cf, key, b"base").unwrap();
    }

    let mut txn = engine.begin(Default::default()).unwrap();
    txn.put_cf(&cf, b"b", b"txn").unwrap();
    txn.put_cf(&cf, b"d", b"txn").unwrap();

    let mut cursor = txn.scan_cf(&cf, Some(b"a"), Some(b"f")).unwrap();
    cursor.seek(b"c").unwrap();

    let rows: Vec<bytes::Bytes> = std::iter::from_fn(|| cursor.next())
        .filter_map(|r| r.ok())
        .map(|(k, _)| k)
        .collect();

    assert_eq!(
        rows,
        vec![
            bytes::Bytes::from_static(b"c"),
            bytes::Bytes::from_static(b"d"),
            bytes::Bytes::from_static(b"e"),
        ]
    );
}

#[test]
fn cf_txn_snapshot_survives_flush() {
    let dir = TempDir::new().unwrap();
    let engine = LsmEngine::open(
        dir.path(),
        LsmOptions {
            write_buffer_size: 64,
            max_write_buffer_number: 2,
            ..Default::default()
        },
    )
    .unwrap();
    let cf = engine.create_column_family("cf1", opts()).unwrap();

    // Write enough data to guarantee at least one background flush, then sync.
    for i in 0..50u8 {
        engine.put_cf(&cf, &[i], &[i + 100]).unwrap();
    }
    engine.sync().unwrap();

    // Begin a transaction after the flush.
    let txn = engine.begin(Default::default()).unwrap();

    // Overwrite every key through the engine.
    for i in 0..50u8 {
        engine.put_cf(&cf, &[i], &[i + 200]).unwrap();
    }

    // The transaction's pinned view must still see the flushed values.
    for i in 0..50u8 {
        assert_eq!(
            txn.get_cf(&cf, &[i]).unwrap(),
            Some(bytes::Bytes::from(vec![i + 100]))
        );
    }
}
