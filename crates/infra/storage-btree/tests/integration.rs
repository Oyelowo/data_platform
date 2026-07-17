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
