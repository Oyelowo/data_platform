//! Boundary-case conformance tests.

use bytes::Bytes;
use storage_traits::{Engine, Error, Transaction, TxnOptions};

/// Run all boundary conformance tests against `factory`.
pub fn run<E, F>(factory: &F)
where
    E: Engine,
    F: Fn() -> E,
{
    empty_key(factory);
    binary_keys(factory);
    unicode_values(factory);
    null_value(factory);
    get_deleted(factory);
}

fn empty_key<E: Engine, F: Fn() -> E>(factory: &F) {
    let engine = factory();
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"", b"empty-key").unwrap();
    tx.commit().unwrap();

    assert_eq!(
        engine.get(b"").unwrap(),
        Some(Bytes::from_static(b"empty-key"))
    );
}

fn binary_keys<E: Engine, F: Fn() -> E>(factory: &F) {
    let engine = factory();
    let key = vec![0x00, 0xFF, 0x10, 0x20];

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(&key, b"binary").unwrap();
    tx.commit().unwrap();

    assert_eq!(
        engine.get(&key).unwrap(),
        Some(Bytes::from_static(b"binary"))
    );
}

fn unicode_values<E: Engine, F: Fn() -> E>(factory: &F) {
    let engine = factory();
    let value = "🦀 Rust 日本語".as_bytes();

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"unicode", value).unwrap();
    tx.commit().unwrap();

    assert_eq!(
        engine.get(b"unicode").unwrap(),
        Some(Bytes::copy_from_slice(value))
    );
}

fn null_value<E: Engine, F: Fn() -> E>(factory: &F) {
    let engine = factory();
    // Empty value and missing key must be distinguishable.
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"empty", b"").unwrap();
    tx.commit().unwrap();

    assert_eq!(engine.get(b"empty").unwrap(), Some(Bytes::new()));
    assert_eq!(engine.get(b"missing").unwrap(), None);
}

fn get_deleted<E: Engine, F: Fn() -> E>(factory: &F) {
    let engine = factory();
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"a", b"1").unwrap();
    tx.commit().unwrap();

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.delete(b"a").unwrap();
    tx.commit().unwrap();

    assert_eq!(engine.get(b"a").unwrap(), None);
}

/// Assert that a result is an `OutOfBounds` error.
pub fn assert_out_of_bounds<T>(result: Result<T, Error>, kind: storage_traits::BoundKind) {
    match result {
        Err(Error::OutOfBounds { kind: k, .. }) => assert_eq!(k, kind),
        Ok(_) => panic!("expected OutOfBounds error, got Ok"),
        Err(other) => panic!("expected OutOfBounds error, got {other}"),
    }
}
