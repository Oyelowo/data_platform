//! Integration tests for `storage-bwtree`.

use std::sync::Arc;
use std::thread;

use bytes::Bytes;
use storage_bwtree::{BwTreeEngine, BwTreeOptions};
use storage_traits::{Cursor, Engine, Transaction, TxnOptions};

fn open() -> (BwTreeEngine, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let engine = BwTreeEngine::open(dir.path(), BwTreeOptions::default()).unwrap();
    (engine, dir)
}

#[test]
fn basic_crud() {
    let (engine, _dir) = open();
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"a", b"1").unwrap();
    tx.commit().unwrap();

    assert_eq!(engine.get(b"a").unwrap(), Some(Bytes::from_static(b"1")));

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"a", b"2").unwrap();
    tx.commit().unwrap();

    assert_eq!(engine.get(b"a").unwrap(), Some(Bytes::from_static(b"2")));

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.delete(b"a").unwrap();
    tx.commit().unwrap();

    assert_eq!(engine.get(b"a").unwrap(), None);
}

#[test]
fn ordering_and_scan() {
    let (engine, _dir) = open();
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"c", b"3").unwrap();
    tx.put(b"a", b"1").unwrap();
    tx.put(b"b", b"2").unwrap();
    tx.commit().unwrap();

    let cursor = engine.scan(Some(b"a"), Some(b"d")).unwrap();
    let keys: Vec<_> = cursor
        .map(|r| r.unwrap().0)
        .map(|k| String::from_utf8(k.to_vec()).unwrap())
        .collect();
    assert_eq!(keys, vec!["a", "b", "c"]);
}

#[test]
fn seek_existing_and_missing() {
    let (engine, _dir) = open();
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"a", b"1").unwrap();
    tx.put(b"c", b"3").unwrap();
    tx.commit().unwrap();

    let mut cursor = engine.scan(None, None).unwrap();
    cursor.seek(b"b").unwrap();
    assert_eq!(cursor.next().unwrap().unwrap().0, Bytes::from_static(b"c"));
}

#[test]
fn empty_key_and_value() {
    let (engine, _dir) = open();
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"", b"empty-key").unwrap();
    tx.put(b"empty-value", b"").unwrap();
    tx.commit().unwrap();

    assert_eq!(
        engine.get(b"").unwrap(),
        Some(Bytes::from_static(b"empty-key"))
    );
    assert_eq!(engine.get(b"empty-value").unwrap(), Some(Bytes::new()));
}

#[test]
fn large_value() {
    let (engine, _dir) = open();
    let value = vec![0xABu8; 1_048_576];
    let value_bytes = Bytes::from(value.clone());

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"large", &value).unwrap();
    tx.commit().unwrap();

    assert_eq!(engine.get(b"large").unwrap(), Some(value_bytes));
}

#[test]
fn reopen_and_recovery() {
    let (engine, dir) = open();
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    for i in 0..100u8 {
        tx.put(&[i], &[i + 100]).unwrap();
    }
    tx.commit().unwrap();
    engine.sync().unwrap();

    // Re-open the engine in the same directory.
    let engine2 = BwTreeEngine::open(dir.path(), BwTreeOptions::default()).unwrap();
    for i in 0..100u8 {
        assert_eq!(engine2.get(&[i]).unwrap(), Some(Bytes::from(vec![i + 100])));
    }
}

#[test]
fn concurrent_readers_and_writers() {
    let (engine, _dir) = open();
    let engine = Arc::new(engine);

    let writer = {
        let engine = Arc::clone(&engine);
        thread::spawn(move || {
            for i in 0..500 {
                let key = format!("key{:03}", i);
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
                let _ = engine.get(b"key000");
            }
        })
    };

    writer.join().unwrap();
    reader.join().unwrap();
}

#[test]
fn concurrent_multi_writer_stress() {
    let (engine, _dir) = open();
    let engine = Arc::new(engine);
    let threads: Vec<_> = (0..8)
        .map(|t| {
            let engine = Arc::clone(&engine);
            thread::spawn(move || {
                for i in 0..100 {
                    let key = format!("t{}-i{}", t, i);
                    let value = format!("v{}", i);
                    let mut tx = engine.begin(TxnOptions::default()).unwrap();
                    tx.put(key.as_bytes(), value.as_bytes()).unwrap();
                    tx.commit().unwrap();
                }
            })
        })
        .collect();

    for handle in threads {
        handle.join().unwrap();
    }

    for t in 0..8 {
        for i in 0..100 {
            let key = format!("t{}-i{}", t, i);
            let value = format!("v{}", i);
            assert_eq!(
                engine.get(key.as_bytes()).unwrap(),
                Some(Bytes::from(value))
            );
        }
    }
}

#[test]
fn large_dataset_with_inner_splits_survives_reopen() {
    let (engine, dir) = open();
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    for i in 0..5_000 {
        let key = format!("{:08}", i);
        tx.put(key.as_bytes(), b"v").unwrap();
    }
    tx.commit().unwrap();
    engine.sync().unwrap();

    let engine2 = BwTreeEngine::open(dir.path(), BwTreeOptions::default()).unwrap();
    for i in 0..5_000 {
        let key = format!("{:08}", i);
        assert_eq!(
            engine2.get(key.as_bytes()).unwrap(),
            Some(Bytes::from_static(b"v")),
            "missing key {}",
            key
        );
    }
}

#[test]
fn read_only_transaction_rejects_write() {
    let (engine, _dir) = open();
    let mut tx = engine.begin(TxnOptions::read_only()).unwrap();
    assert!(tx.put(b"a", b"1").is_err());
}

#[test]
fn rollback_discards_writes() {
    let (engine, _dir) = open();
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"a", b"1").unwrap();
    tx.rollback().unwrap();
    assert_eq!(engine.get(b"a").unwrap(), None);
}
