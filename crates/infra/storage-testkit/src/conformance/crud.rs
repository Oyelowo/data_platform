//! CRUD conformance tests.

use bytes::Bytes;
use storage_traits::{Engine, Error, Transaction, TxnOptions};

/// Run all CRUD conformance tests against `factory`.
pub fn run<E, F>(factory: &F)
where
    E: Engine,
    F: Fn() -> E,
{
    put_then_get(factory);
    put_overwrites(factory);
    delete_removes(factory);
    delete_missing_is_ok(factory);
    get_missing_is_none(factory);
    put_empty_value(factory);
    put_large_value(factory);
    put_many(factory);
}

fn put_then_get<E: Engine, F: Fn() -> E>(factory: &F) {
    let engine = factory();
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"a", b"1").unwrap();
    tx.commit().unwrap();

    assert_eq!(engine.get(b"a").unwrap(), Some(Bytes::from_static(b"1")));
}

fn put_overwrites<E: Engine, F: Fn() -> E>(factory: &F) {
    let engine = factory();
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"a", b"1").unwrap();
    tx.commit().unwrap();

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"a", b"2").unwrap();
    tx.commit().unwrap();

    assert_eq!(engine.get(b"a").unwrap(), Some(Bytes::from_static(b"2")));
}

fn delete_removes<E: Engine, F: Fn() -> E>(factory: &F) {
    let engine = factory();
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"a", b"1").unwrap();
    tx.commit().unwrap();

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.delete(b"a").unwrap();
    tx.commit().unwrap();

    assert_eq!(engine.get(b"a").unwrap(), None);
}

fn delete_missing_is_ok<E: Engine, F: Fn() -> E>(factory: &F) {
    let engine = factory();
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.delete(b"missing").unwrap();
    tx.commit().unwrap();
}

fn get_missing_is_none<E: Engine, F: Fn() -> E>(factory: &F) {
    let engine = factory();
    assert_eq!(engine.get(b"missing").unwrap(), None);
}

fn put_empty_value<E: Engine, F: Fn() -> E>(factory: &F) {
    let engine = factory();
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"a", b"").unwrap();
    tx.commit().unwrap();

    assert_eq!(engine.get(b"a").unwrap(), Some(Bytes::new()));
}

fn put_large_value<E: Engine, F: Fn() -> E>(factory: &F) {
    let engine = factory();
    let value = vec![0xABu8; 1_048_576]; // 1 MiB
    let value_bytes = Bytes::from(value.clone());

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"large", &value).unwrap();
    tx.commit().unwrap();

    assert_eq!(engine.get(b"large").unwrap(), Some(value_bytes));
}

fn put_many<E: Engine, F: Fn() -> E>(factory: &F) {
    let engine = factory();
    let count = 1_000;

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    for i in 0..count {
        let key = format!("key{:08}", i);
        let value = format!("value{}", i);
        tx.put(key.as_bytes(), value.as_bytes()).unwrap();
    }
    tx.commit().unwrap();

    for i in 0..count {
        let key = format!("key{:08}", i);
        let value = format!("value{}", i);
        assert_eq!(
            engine.get(key.as_bytes()).unwrap(),
            Some(Bytes::from(value))
        );
    }
}

/// Assert that a result is an `OutOfBounds` error for the given kind.
pub fn assert_out_of_bounds<T>(result: Result<T, Error>, kind: storage_traits::BoundKind) {
    match result {
        Err(Error::OutOfBounds { kind: k, .. }) => assert_eq!(k, kind),
        Ok(_) => panic!("expected OutOfBounds error, got Ok"),
        Err(other) => panic!("expected OutOfBounds error, got {other}"),
    }
}
