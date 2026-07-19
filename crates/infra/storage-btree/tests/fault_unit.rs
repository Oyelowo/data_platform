//! Targeted fault-injection tests for every I/O boundary.
//!
//! These tests use a [`FaultyBackend`] to inject deterministic faults at
//! specific semantic boundaries and verify the engine's error handling and
//! recovery behaviour.

use std::io::ErrorKind;
use std::sync::Arc;

use bytes::Bytes;
use storage_btree::{
    Boundary, BtreeEngine, BtreeOptions, FaultRule, FaultSchedule, FaultyBackend, OpFamily,
    RealBackend, StorageBackend,
};
use storage_traits::{Engine, Transaction, TxnOptions};

fn options_with_fault(rules: Vec<FaultRule>) -> BtreeOptions {
    BtreeOptions {
        fault_schedule: Some(FaultSchedule { seed: 1, rules }),
        ..Default::default()
    }
}

#[test]
fn fail_page_write_returns_io_error_and_no_corruption() {
    let dir = tempfile::tempdir().unwrap();
    let options = options_with_fault(vec![FaultRule::FailNth {
        op: OpFamily::PageWrite,
        n: 1,
        error: ErrorKind::PermissionDenied,
    }]);
    let engine = BtreeEngine::open(dir.path(), options).unwrap();

    let mut txn = engine.begin(TxnOptions::default()).unwrap();
    txn.put(b"k", b"v").unwrap();
    txn.commit().unwrap();

    // Page writes happen when the buffer pool flushes dirty pages during a
    // checkpoint, so the fault is surfaced by engine.sync().
    assert!(
        engine.sync().is_err(),
        "page write fault should fail checkpoint"
    );

    drop(engine);
    let engine2 = BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap();
    engine2.check_integrity().unwrap();
}

#[test]
fn fail_page_sync_is_fatal_and_consistent_on_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let options = options_with_fault(vec![FaultRule::FailNth {
        op: OpFamily::PageSync,
        n: 1,
        error: ErrorKind::Other,
    }]);
    let engine = BtreeEngine::open(dir.path(), options).unwrap();

    let mut txn = engine.begin(TxnOptions::default()).unwrap();
    txn.put(b"k", b"v").unwrap();
    txn.commit().unwrap();
    assert!(
        engine.sync().is_err(),
        "page sync fault should fail checkpoint"
    );

    drop(engine);
    let engine2 = BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap();
    engine2.check_integrity().unwrap();
}

#[test]
fn partial_page_write_is_detected_on_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let options = BtreeOptions {
        page_size: 512,
        max_inline_value_size: 64,
        min_cells: Some(1),
        fault_schedule: Some(FaultSchedule {
            seed: 2,
            rules: vec![FaultRule::PartialWriteNth {
                op: OpFamily::PageWrite,
                n: 1,
                truncate_to: 64,
            }],
        }),
        ..Default::default()
    };
    let engine = BtreeEngine::open(dir.path(), options).unwrap();

    let value = vec![0xABu8; 100];
    let mut txn = engine.begin(TxnOptions::default()).unwrap();
    txn.put(b"k", &value).unwrap();
    txn.commit().unwrap();

    // Force a checkpoint so the truncated page write lands on disk. The
    // partial page is detected as corrupt on the next open.
    engine.sync().unwrap();
    drop(engine);

    let result = BtreeEngine::open(dir.path(), BtreeOptions::default());
    assert!(
        result.is_err() || result.as_ref().unwrap().check_integrity().is_err(),
        "torn page should be detected after reopen"
    );
}

#[test]
fn fail_value_log_append_errors_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let options = options_with_fault(vec![FaultRule::FailNth {
        op: OpFamily::ValueLogAppend,
        n: 1,
        error: ErrorKind::PermissionDenied,
    }]);
    let engine = BtreeEngine::open(dir.path(), options).unwrap();

    let value = vec![0xCDu8; 2048];
    let mut txn = engine.begin(TxnOptions::default()).unwrap();
    assert!(
        txn.put(b"big", &value).is_err(),
        "value-log append fault should fail put"
    );

    drop(engine);
    let engine2 = BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap();
    assert_eq!(engine2.get(b"big").unwrap(), None);
    engine2.check_integrity().unwrap();
}

#[test]
fn fail_meta_write_preserves_previous_meta() {
    let dir = tempfile::tempdir().unwrap();
    let engine = BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap();

    // Establish a durable checkpoint with one key.
    let mut txn = engine.begin(TxnOptions::default()).unwrap();
    txn.put(b"before", b"1").unwrap();
    txn.commit().unwrap();
    engine.sync().unwrap();
    drop(engine);

    // Reopen with a fault injected into the next META write.
    let options = options_with_fault(vec![FaultRule::FailNth {
        op: OpFamily::MetaWrite,
        n: 1,
        error: ErrorKind::PermissionDenied,
    }]);
    let engine = BtreeEngine::open(dir.path(), options).unwrap();

    let mut txn = engine.begin(TxnOptions::default()).unwrap();
    txn.put(b"after", b"2").unwrap();
    txn.commit().unwrap();
    assert!(
        engine.sync().is_err(),
        "META write fault should fail checkpoint"
    );

    drop(engine);
    let engine2 = BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap();
    // The previous META is still usable, and WAL replay makes both keys visible.
    assert_eq!(
        engine2.get(b"before").unwrap(),
        Some(Bytes::from_static(b"1"))
    );
    assert_eq!(
        engine2.get(b"after").unwrap(),
        Some(Bytes::from_static(b"2"))
    );
    engine2.check_integrity().unwrap();
}

#[test]
fn fail_meta_dir_sync_reports_error() {
    let dir = tempfile::tempdir().unwrap();
    let options = options_with_fault(vec![FaultRule::FailNth {
        op: OpFamily::DirSync,
        n: 1,
        error: ErrorKind::PermissionDenied,
    }]);
    let engine = BtreeEngine::open(dir.path(), options).unwrap();

    let mut txn = engine.begin(TxnOptions::default()).unwrap();
    txn.put(b"k", b"v").unwrap();
    txn.commit().unwrap();
    assert!(
        engine.sync().is_err(),
        "META dir-sync fault should fail checkpoint"
    );

    drop(engine);
    let engine2 = BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap();
    engine2.check_integrity().unwrap();
}

#[test]
fn fail_value_log_sync_propagates_durability_error() {
    let dir = tempfile::tempdir().unwrap();
    let options = options_with_fault(vec![FaultRule::FailNth {
        op: OpFamily::ValueLogSync,
        n: 1,
        error: ErrorKind::PermissionDenied,
    }]);
    let engine = BtreeEngine::open(dir.path(), options).unwrap();

    let value = vec![0xEFu8; 2048];
    let mut txn = engine.begin(TxnOptions::default()).unwrap();
    txn.put(b"big", &value).unwrap();
    assert!(
        txn.commit().is_err(),
        "value-log sync fault should fail commit"
    );

    drop(engine);
    let engine2 = BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap();
    engine2.check_integrity().unwrap();
}

#[test]
fn fail_wal_sync_still_uses_storage_wal_fault_config() {
    // WAL faults remain the responsibility of storage-wal; verify the engine
    // still wires the FaultConfig through correctly.
    let dir = tempfile::tempdir().unwrap();
    let options = BtreeOptions {
        wal_fault_config: Some(storage_wal::FaultConfig {
            fail_sync_every: Some(1),
            ..Default::default()
        }),
        ..Default::default()
    };
    let engine = BtreeEngine::open(dir.path(), options).unwrap();

    let mut txn = engine.begin(TxnOptions::default()).unwrap();
    txn.put(b"k", b"v").unwrap();
    assert!(txn.commit().is_err(), "WAL sync fault should fail commit");
    let _ = engine.close();
}

#[test]
fn corrupt_value_log_read_detects_length_mismatch() {
    let dir = tempfile::tempdir().unwrap();
    let engine = BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap();

    let value = vec![0x12u8; 2048];
    let mut txn = engine.begin(TxnOptions::default()).unwrap();
    txn.put(b"big", &value).unwrap();
    txn.commit().unwrap();
    engine.sync().unwrap();
    drop(engine);

    // Reopen through a backend that corrupts the first value-log read.
    let backend = Arc::new(FaultyBackend::new(
        Arc::new(RealBackend),
        FaultSchedule {
            seed: 3,
            rules: vec![FaultRule::CorruptReadNth {
                op: OpFamily::ValueLogRead,
                n: 1,
                offset: 0,
                len: 4,
                xor: 0xFF,
            }],
        },
    ));
    let options = BtreeOptions {
        backend: Some(backend),
        ..Default::default()
    };
    let engine = BtreeEngine::open(dir.path(), options).unwrap();
    assert!(
        engine.get(b"big").is_err(),
        "corrupted value-log length should be detected"
    );
}

#[test]
fn drop_appends_loses_unsynced_value() {
    let dir = tempfile::tempdir().unwrap();
    let options = options_with_fault(vec![FaultRule::DropAppends]);
    let engine = BtreeEngine::open(dir.path(), options).unwrap();

    let value = vec![0x34u8; 2048];
    let mut txn = engine.begin(TxnOptions::default()).unwrap();
    txn.put(b"big", &value).unwrap();
    txn.commit().unwrap();
    // The append returned Ok but the bytes were dropped. Crash to release the
    // WAL lock, then forget the engine without an explicit fsync so the value
    // remains unrecoverable.
    engine.crash();
    std::mem::forget(engine);

    let engine2 = BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap();
    // The dropped append leaves a dangling value-log reference; the engine
    // detects the missing value.
    assert!(engine2.get(b"big").is_err());
    engine2.check_integrity().unwrap();
}

#[test]
fn operation_log_records_boundaries() {
    let dir = tempfile::tempdir().unwrap();
    let backend = Arc::new(FaultyBackend::new(
        Arc::new(RealBackend),
        FaultSchedule::default(),
    ));
    let options = BtreeOptions {
        backend: Some(backend.clone()),
        ..Default::default()
    };
    let engine = BtreeEngine::open(dir.path(), options).unwrap();

    let mut txn = engine.begin(TxnOptions::default()).unwrap();
    // Use a value large enough to go through the value log.
    txn.put(b"k", &[1u8; 2048]).unwrap();
    txn.commit().unwrap();
    // Force a page flush and checkpoint so PageWrite/MetaWrite boundaries are
    // also logged.
    let _ = engine.sync();

    let log = backend.operation_log();
    assert!(
        log.iter().any(|(b, _)| matches!(b, Boundary::PageWrite(_))),
        "page writes should be logged"
    );
    assert!(
        log.iter()
            .any(|(b, _)| matches!(b, Boundary::ValueLogAppend)),
        "value-log appends should be logged"
    );
    assert!(
        log.iter()
            .any(|(b, _)| matches!(b, Boundary::MetaWriteTemp)),
        "META writes should be logged"
    );
}
