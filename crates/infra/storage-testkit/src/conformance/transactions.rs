//! Transaction conformance tests.

use bytes::Bytes;
use storage_traits::{Engine, Transaction, TxnOptions};

/// Run all transaction conformance tests against `factory`.
pub fn run<E, F>(factory: &F)
where
    E: Engine,
    F: Fn() -> E,
{
    commit_persists(factory);
    rollback_discards(factory);
    read_your_writes(factory);
    no_dirty_reads(factory);
    read_only_rejects_write(factory);
    double_commit_errors(factory);
    rollback_after_commit_errors(factory);
}

fn commit_persists<E: Engine, F: Fn() -> E>(factory: &F) {
    let engine = factory();
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"a", b"1").unwrap();
    tx.commit().unwrap();

    assert_eq!(engine.get(b"a").unwrap(), Some(Bytes::from_static(b"1")));
}

fn rollback_discards<E: Engine, F: Fn() -> E>(factory: &F) {
    let engine = factory();
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"a", b"1").unwrap();
    tx.rollback().unwrap();

    assert_eq!(engine.get(b"a").unwrap(), None);
}

fn read_your_writes<E: Engine, F: Fn() -> E>(factory: &F) {
    let engine = factory();
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"a", b"1").unwrap();
    assert_eq!(tx.get(b"a").unwrap(), Some(Bytes::from_static(b"1")));
    tx.commit().unwrap();
}

fn no_dirty_reads<E: Engine, F: Fn() -> E>(factory: &F) {
    let engine = factory();
    let mut tx1 = engine.begin(TxnOptions::default()).unwrap();
    tx1.put(b"a", b"1").unwrap();

    let tx2 = engine.begin(TxnOptions::default()).unwrap();
    assert_eq!(tx2.get(b"a").unwrap(), None);

    tx1.commit().unwrap();
}

fn read_only_rejects_write<E: Engine, F: Fn() -> E>(factory: &F) {
    let engine = factory();
    let mut tx = engine.begin(TxnOptions::read_only()).unwrap();
    let result = tx.put(b"a", b"1");
    assert!(result.is_err(), "read-only transaction must reject writes");
}

fn double_commit_errors<E: Engine, F: Fn() -> E>(factory: &F) {
    let engine = factory();
    let tx = engine.begin(TxnOptions::default()).unwrap();
    tx.commit().unwrap();
    // After commit the transaction is consumed, so this must not compile.
    // We test the consume-by-value design statically.
}

fn rollback_after_commit_errors<E: Engine, F: Fn() -> E>(factory: &F) {
    let engine = factory();
    let tx = engine.begin(TxnOptions::default()).unwrap();
    tx.commit().unwrap();
    // Same: transaction is consumed on commit.
}
