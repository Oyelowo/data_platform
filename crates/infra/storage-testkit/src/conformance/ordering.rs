//! Ordering and scan conformance tests.

use bytes::Bytes;
use storage_traits::{Cursor, Engine, Transaction, TxnOptions};

/// Run all ordering conformance tests against `factory`.
pub fn run<E, F>(factory: &F)
where
    E: Engine,
    F: Fn() -> E,
{
    scan_sorted(factory);
    scan_prefix(factory);
    scan_empty_range(factory);
    scan_unbounded(factory);
    seek_existing(factory);
    seek_greater(factory);
    seek_past_end(factory);
    scan_after_delete(factory);
}

fn scan_sorted<E: Engine, F: Fn() -> E>(factory: &F) {
    let engine = factory();
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

fn scan_prefix<E: Engine, F: Fn() -> E>(factory: &F) {
    let engine = factory();
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"prefix:one", b"1").unwrap();
    tx.put(b"prefix:two", b"2").unwrap();
    tx.put(b"other", b"3").unwrap();
    tx.commit().unwrap();

    let cursor = engine.scan(Some(b"prefix:"), Some(b"prefix;")).unwrap();
    let keys: Vec<_> = cursor
        .map(|r| r.unwrap().0)
        .map(|k| String::from_utf8(k.to_vec()).unwrap())
        .collect();

    assert_eq!(keys, vec!["prefix:one", "prefix:two"]);
}

fn scan_empty_range<E: Engine, F: Fn() -> E>(factory: &F) {
    let engine = factory();
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"a", b"1").unwrap();
    tx.put(b"c", b"3").unwrap();
    tx.commit().unwrap();

    let mut cursor = engine.scan(Some(b"b"), Some(b"c")).unwrap();
    assert!(cursor.next().is_none());
}

fn scan_unbounded<E: Engine, F: Fn() -> E>(factory: &F) {
    let engine = factory();
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"a", b"1").unwrap();
    tx.put(b"b", b"2").unwrap();
    tx.commit().unwrap();

    let cursor = engine.scan(None, None).unwrap();
    let keys: Vec<_> = cursor
        .map(|r| r.unwrap().0)
        .map(|k| String::from_utf8(k.to_vec()).unwrap())
        .collect();

    assert_eq!(keys, vec!["a", "b"]);
}

fn seek_existing<E: Engine, F: Fn() -> E>(factory: &F) {
    let engine = factory();
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"a", b"1").unwrap();
    tx.put(b"c", b"3").unwrap();
    tx.commit().unwrap();

    let mut cursor = engine.scan(None, None).unwrap();
    cursor.seek(b"c").unwrap();
    assert_eq!(cursor.next().unwrap().unwrap().0, Bytes::from_static(b"c"));
}

fn seek_greater<E: Engine, F: Fn() -> E>(factory: &F) {
    let engine = factory();
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"a", b"1").unwrap();
    tx.put(b"c", b"3").unwrap();
    tx.commit().unwrap();

    let mut cursor = engine.scan(None, None).unwrap();
    cursor.seek(b"b").unwrap();
    assert_eq!(cursor.next().unwrap().unwrap().0, Bytes::from_static(b"c"));
}

fn seek_past_end<E: Engine, F: Fn() -> E>(factory: &F) {
    let engine = factory();
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"a", b"1").unwrap();
    tx.commit().unwrap();

    let mut cursor = engine.scan(None, None).unwrap();
    cursor.seek(b"z").unwrap();
    assert!(cursor.next().is_none());
}

fn scan_after_delete<E: Engine, F: Fn() -> E>(factory: &F) {
    let engine = factory();
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"a", b"1").unwrap();
    tx.put(b"b", b"2").unwrap();
    tx.put(b"c", b"3").unwrap();
    tx.commit().unwrap();

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.delete(b"b").unwrap();
    tx.commit().unwrap();

    let cursor = engine.scan(Some(b"a"), Some(b"d")).unwrap();
    let keys: Vec<_> = cursor
        .map(|r| r.unwrap().0)
        .map(|k| String::from_utf8(k.to_vec()).unwrap())
        .collect();

    assert_eq!(keys, vec!["a", "c"]);
}
