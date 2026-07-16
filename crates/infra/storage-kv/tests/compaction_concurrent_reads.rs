//! Reads observe a consistent view while compaction runs in the background.

use std::sync::{Arc, Barrier};
use std::thread;

use storage_kv::{LsmEngine, LsmOptions};
use storage_traits::{Engine, Transaction};

fn opts() -> LsmOptions {
    LsmOptions {
        write_buffer_size: 64,
        max_write_buffer_number: 2,
        level0_file_num_compaction_trigger: 2,
        level0_slowdown_writes_trigger: 100,
        level0_stop_writes_trigger: 200,
        target_file_size_base: 256,
        max_bytes_for_level_base: 256,
        ..Default::default()
    }
}

/// A read snapshot started before a compaction must see the same values after
/// the compaction finishes, even though the on-disk file set has changed.
#[test]
fn snapshot_survives_background_compaction() {
    let dir = tempfile::tempdir().unwrap();
    let opts = opts();
    let engine = Arc::new(LsmEngine::open(dir.path(), opts.clone()).unwrap());

    // Seed many keys so a compaction will move significant data.
    for round in 0..5u8 {
        for i in 0..100u8 {
            engine.put(&[i], &[round, i]).unwrap();
        }
    }
    engine.sync().unwrap();

    // Start a read-only transaction.  Its snapshot is the current Version at
    // this point, which references files that compaction will delete.
    let txn = engine.begin(Default::default()).unwrap();

    // Sanity check: the snapshot must already see round 4 before any later
    // writes or compactions run.
    for i in 0..100u8 {
        let expected = bytes::Bytes::from(vec![4, i]);
        let actual = txn.get(&[i]).unwrap();
        if actual != Some(expected.clone()) {
            panic!(
                "snapshot captured wrong value for key {} before later writes: {:?} (seq={})",
                i,
                actual,
                txn.snapshot_sequence()
            );
        }
    }

    // Force more overwrites to trigger compactions while the snapshot is held.
    for round in 5..10u8 {
        for i in 0..100u8 {
            engine.put(&[i], &[round, i]).unwrap();
        }
    }
    engine.sync().unwrap();

    // The snapshot must still observe the values from the version it captured.
    let mut failures = Vec::new();
    for i in 0..100u8 {
        let expected = bytes::Bytes::from(vec![4, i]);
        let actual = txn.get(&[i]).unwrap();
        if actual != Some(expected.clone()) {
            let outside = engine.get(&[i]).unwrap();
            failures.push((i, actual, outside));
            if failures.len() >= 5 {
                break;
            }
        }
    }
    if !failures.is_empty() {
        panic!(
            "snapshot mismatch (seq={}): {:?}",
            txn.snapshot_sequence(),
            failures
        );
    }
}

/// Concurrent scans and writes do not observe torn compaction state: every
/// reader sees a single `Version` for the lifetime of its cursor.
#[test]
fn concurrent_scans_during_compaction() {
    let dir = tempfile::tempdir().unwrap();
    let opts = opts();
    let engine = Arc::new(LsmEngine::open(dir.path(), opts.clone()).unwrap());

    // Initial data.
    for i in 0..200u16 {
        engine.put(&i.to_be_bytes(), &i.to_le_bytes()).unwrap();
    }
    engine.sync().unwrap();

    let barrier = Arc::new(Barrier::new(3));
    let mut handles = Vec::new();

    // Writer thread: overwrite keys repeatedly to trigger compactions.
    let writer_engine = Arc::clone(&engine);
    let writer_barrier = Arc::clone(&barrier);
    handles.push(thread::spawn(move || {
        writer_barrier.wait();
        for round in 0..10u8 {
            for i in 0..200u16 {
                writer_engine
                    .put(&i.to_be_bytes(), &[round; 2])
                    .unwrap();
            }
        }
        writer_engine.sync().unwrap();
    }));

    // Two reader threads: each performs full scans and checks that keys are
    // monotonically ordered and that every key appears exactly once.
    for _ in 0..2 {
        let reader_engine = Arc::clone(&engine);
        let reader_barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            reader_barrier.wait();
            for _ in 0..10 {
                let mut last_key: Option<Vec<u8>> = None;
                let mut count = 0usize;
                let cursor = reader_engine.scan(None, None).unwrap();
                for item in cursor {
                    let (k, _v) = item.unwrap();
                    if let Some(ref last) = last_key {
                        assert!(k.as_ref() > last.as_slice(), "scan must be sorted");
                    }
                    last_key = Some(k.to_vec());
                    count += 1;
                }
                // There should be exactly 200 live keys at all times.
                assert_eq!(count, 200);
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }
}
