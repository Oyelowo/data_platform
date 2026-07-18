//! Crash-at-boundary harness tests.
//!
//! These tests run a workload through a [`FaultyBackend`], simulate a power
//! loss by dropping buffered writes, then reopen with [`RealBackend`] and
//! verify recovery invariants.

use bytes::Bytes;
use std::sync::Arc;
use storage_btree::{BtreeEngine, BtreeOptions, FaultSchedule, FaultyBackend, RealBackend};
use storage_traits::{Engine, Transaction, TxnOptions};

fn crash_options() -> BtreeOptions {
    // An empty schedule is enough to activate a FaultyBackend so that crash()
    // can truncate files to their last-synced length.
    BtreeOptions {
        fault_schedule: Some(FaultSchedule::default()),
        ..Default::default()
    }
}

#[test]
fn crash_after_commit_preserves_committed_data() {
    let dir = tempfile::tempdir().unwrap();
    let engine = BtreeEngine::open(dir.path(), crash_options()).unwrap();

    let mut txn = engine.begin(TxnOptions::default()).unwrap();
    txn.put(b"committed", b"yes").unwrap();
    txn.commit().unwrap();

    engine.crash();
    std::mem::forget(engine);

    let engine2 = BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap();
    assert_eq!(
        engine2.get(b"committed").unwrap(),
        Some(Bytes::from_static(b"yes"))
    );
    engine2.check_integrity().unwrap();
}

#[test]
fn crash_without_commit_loses_uncommitted_data() {
    let dir = tempfile::tempdir().unwrap();
    let engine = BtreeEngine::open(dir.path(), crash_options()).unwrap();

    let mut txn = engine.begin(TxnOptions::default()).unwrap();
    txn.put(b"uncommitted", b"lost").unwrap();
    // No commit: the buffered WAL record is dropped on crash.

    engine.crash();
    std::mem::forget(engine);

    let engine2 = BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap();
    assert_eq!(engine2.get(b"uncommitted").unwrap(), None);
    engine2.check_integrity().unwrap();
}

#[test]
fn crash_after_delete_commit_persists_deletion() {
    let dir = tempfile::tempdir().unwrap();
    let engine = BtreeEngine::open(dir.path(), crash_options()).unwrap();

    let mut txn = engine.begin(TxnOptions::default()).unwrap();
    txn.put(b"gone", b"value").unwrap();
    txn.commit().unwrap();

    let mut txn = engine.begin(TxnOptions::default()).unwrap();
    txn.delete(b"gone").unwrap();
    txn.commit().unwrap();

    engine.crash();
    std::mem::forget(engine);

    let engine2 = BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap();
    assert_eq!(engine2.get(b"gone").unwrap(), None);
    engine2.check_integrity().unwrap();
}

#[test]
fn crash_with_large_value_preserves_oracle() {
    let dir = tempfile::tempdir().unwrap();
    let engine = BtreeEngine::open(dir.path(), crash_options()).unwrap();

    let small = b"small".to_vec();
    let large = vec![0xABu8; 1_048_576];

    let mut txn = engine.begin(TxnOptions::default()).unwrap();
    txn.put(b"s", &small).unwrap();
    txn.put(b"l", &large).unwrap();
    txn.commit().unwrap();

    engine.crash();
    std::mem::forget(engine);

    let engine2 = BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap();
    assert_eq!(engine2.get(b"s").unwrap(), Some(Bytes::from(small)));
    assert_eq!(engine2.get(b"l").unwrap(), Some(Bytes::from(large)));
    engine2.check_integrity_with_value_log().unwrap();
}

#[test]
fn crash_after_partial_workload_matches_oracle() {
    let dir = tempfile::tempdir().unwrap();
    let engine = BtreeEngine::open(dir.path(), crash_options()).unwrap();

    let mut oracle = std::collections::BTreeMap::new();
    for i in 0u8..10 {
        let key = vec![i];
        let value = vec![i + 100];
        let mut txn = engine.begin(TxnOptions::default()).unwrap();
        txn.put(&key, &value).unwrap();
        txn.commit().unwrap();
        oracle.insert(key, value);
    }

    engine.crash();
    std::mem::forget(engine);

    let engine2 = BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap();
    for (key, value) in &oracle {
        assert_eq!(engine2.get(key).unwrap(), Some(Bytes::from(value.clone())));
    }
    engine2.check_integrity().unwrap();
}

#[test]
fn crash_with_backend_wrapper_is_replayable() {
    // Explicitly construct a FaultyBackend around RealBackend and pass it in,
    // rather than relying on fault_schedule to create one.
    let dir = tempfile::tempdir().unwrap();
    let backend = Arc::new(FaultyBackend::new(
        Arc::new(RealBackend),
        FaultSchedule::default(),
    ));
    let options = BtreeOptions {
        backend: Some(backend),
        ..Default::default()
    };
    let engine = BtreeEngine::open(dir.path(), options).unwrap();

    let mut txn = engine.begin(TxnOptions::default()).unwrap();
    txn.put(b"k", b"v").unwrap();
    txn.commit().unwrap();

    engine.crash();
    std::mem::forget(engine);

    let engine2 = BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap();
    assert_eq!(engine2.get(b"k").unwrap(), Some(Bytes::from_static(b"v")));
    engine2.check_integrity().unwrap();
}
