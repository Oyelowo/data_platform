//! Snapshot-isolation tests for the LSM engine.
//!
//! These tests verify that transactions and point-in-time reads observe a
//! consistent snapshot that is not affected by concurrent writes.

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use storage_kv::{LsmEngine, LsmOptions};
use storage_traits::{Engine, Transaction};
use tempfile::TempDir;

fn opts() -> LsmOptions {
    LsmOptions {
        write_buffer_size: 256,
        ..Default::default()
    }
}

#[test]
fn point_read_snapshot_hides_later_write() {
    let dir = TempDir::new().unwrap();
    let engine = LsmEngine::open(dir.path(), opts()).unwrap();

    engine.put(b"key", b"initial").unwrap();

    let txn = engine.begin(Default::default()).unwrap();
    assert_eq!(
        txn.get(b"key").unwrap(),
        Some(bytes::Bytes::from_static(b"initial"))
    );

    engine.put(b"key", b"updated").unwrap();

    // The existing transaction must still see the value at the time it started.
    assert_eq!(
        txn.get(b"key").unwrap(),
        Some(bytes::Bytes::from_static(b"initial"))
    );

    drop(txn);

    // A new transaction sees the updated value.
    let txn2 = engine.begin(Default::default()).unwrap();
    assert_eq!(
        txn2.get(b"key").unwrap(),
        Some(bytes::Bytes::from_static(b"updated"))
    );
}

#[test]
fn scan_snapshot_isolation() {
    let dir = TempDir::new().unwrap();
    let engine = LsmEngine::open(dir.path(), opts()).unwrap();

    for i in 0..10 {
        engine
            .put(format!("k{}", i).as_bytes(), b"base")
            .unwrap();
    }

    let txn = engine.begin(Default::default()).unwrap();

    // Concurrent modifications.
    engine.put(b"k5", b"updated").unwrap();
    engine.delete(b"k3").unwrap();

    let mut cursor = txn.scan(Some(b"k0"), Some(b"k:")).unwrap();
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
}

#[test]
fn snapshot_is_monotonic_under_concurrent_writes() {
    // Use a tiny write buffer so MemTables are frozen and flushed while readers
    // are active.
    let dir = TempDir::new().unwrap();
    let engine = Arc::new(
        LsmEngine::open(
            dir.path(),
            LsmOptions {
                write_buffer_size: 64,
                ..Default::default()
            },
        )
        .unwrap(),
    );

    let engine_w = engine.clone();
    let writer = thread::spawn(move || {
        for i in 0..1_000usize {
            engine_w
                .put(b"counter", format!("{}", i).as_bytes())
                .unwrap();
        }
    });

    let engine_r = engine.clone();
    let reader = thread::spawn(move || {
        let mut last_seen: Option<usize> = None;
        for _ in 0..100 {
            thread::sleep(Duration::from_millis(1));
            let value = engine_r.get(b"counter").unwrap();
            if let Some(v) = value {
                let n = String::from_utf8(v.to_vec())
                    .unwrap()
                    .parse::<usize>()
                    .unwrap();
                if let Some(last) = last_seen {
                    assert!(
                        n >= last,
                        "monotonic-read violation: saw {} after {}",
                        n,
                        last
                    );
                }
                last_seen = Some(n);
            }
        }
    });

    writer.join().unwrap();
    reader.join().unwrap();

    engine.sync().unwrap();
}

#[test]
fn concurrent_transactions_have_consistent_snapshots() {
    let dir = TempDir::new().unwrap();
    let engine = Arc::new(LsmEngine::open(dir.path(), opts()).unwrap());

    for i in 0..20 {
        engine
            .put(format!("k{}", i).as_bytes(), b"base")
            .unwrap();
    }

    let barrier = Arc::new(std::sync::Barrier::new(3));
    let mut handles = Vec::new();

    for t in 0..3 {
        let engine = engine.clone();
        let barrier = barrier.clone();
        handles.push(thread::spawn(move || {
            barrier.wait();
            let mut txn = engine.begin(Default::default()).unwrap();
            {
                let mut cursor = txn.scan(None, None).unwrap();
                let mut count = 0;
                while let Some(Ok(_)) = cursor.next() {
                    count += 1;
                }
                // Each transaction must see the full set of base keys; it will
                // not see its own writes in this simple snapshot model.
                assert!(
                    count == 20,
                    "thread {} saw {} keys, expected 20",
                    t,
                    count
                );
            }

            // Each thread writes its own unique keys inside the transaction.
            txn.put(format!("txn{}-a", t).as_bytes(), b"1").unwrap();
            txn.put(format!("txn{}-b", t).as_bytes(), b"2").unwrap();
            txn.put(format!("txn{}-c", t).as_bytes(), b"3").unwrap();
            txn.commit().unwrap();
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    engine.sync().unwrap();

    for t in 0..3 {
        assert_eq!(
            engine.get(format!("txn{}-a", t).as_bytes()).unwrap(),
            Some(bytes::Bytes::from_static(b"1"))
        );
        assert_eq!(
            engine.get(format!("txn{}-b", t).as_bytes()).unwrap(),
            Some(bytes::Bytes::from_static(b"2"))
        );
        assert_eq!(
            engine.get(format!("txn{}-c", t).as_bytes()).unwrap(),
            Some(bytes::Bytes::from_static(b"3"))
        );
    }
}
