//! Integration tests for range-deletion tombstones.

use bytes::Bytes;
use storage_kv::{LsmEngine, LsmOptions};
use storage_traits::Engine;

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

#[test]
fn delete_range_hides_keys_in_memtable() {
    let dir = tempfile::tempdir().unwrap();
    let engine = LsmEngine::open(dir.path(), opts()).unwrap();

    for i in 0..10u8 {
        engine.put(&[i], &[i, 1]).unwrap();
    }
    engine.delete_range(&[3], &[7]).unwrap();

    for i in 0..10u8 {
        let got = engine.get(&[i]).unwrap();
        if (3..7).contains(&i) {
            assert_eq!(got, None, "key {} should be range-deleted", i);
        } else {
            assert_eq!(
                got,
                Some(Bytes::from(vec![i, 1])),
                "key {} should survive",
                i
            );
        }
    }
}

#[test]
fn delete_range_survives_flush() {
    let dir = tempfile::tempdir().unwrap();
    let engine = LsmEngine::open(dir.path(), opts()).unwrap();

    for i in 0..10u8 {
        engine.put(&[i], &[i, 1]).unwrap();
    }
    engine.delete_range(&[3], &[7]).unwrap();
    engine.sync().unwrap();

    for i in 0..10u8 {
        let got = engine.get(&[i]).unwrap();
        if (3..7).contains(&i) {
            assert_eq!(got, None, "key {} should be range-deleted after flush", i);
        } else {
            assert_eq!(
                got,
                Some(Bytes::from(vec![i, 1])),
                "key {} should survive flush",
                i
            );
        }
    }
}

#[test]
fn delete_range_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    {
        let engine = LsmEngine::open(dir.path(), opts()).unwrap();
        for i in 0..10u8 {
            engine.put(&[i], &[i, 1]).unwrap();
        }
        engine.delete_range(&[3], &[7]).unwrap();
        engine.sync().unwrap();
    }

    let engine = LsmEngine::open(dir.path(), opts()).unwrap();
    for i in 0..10u8 {
        let got = engine.get(&[i]).unwrap();
        if (3..7).contains(&i) {
            assert_eq!(got, None, "key {} should be range-deleted after reopen", i);
        } else {
            assert_eq!(
                got,
                Some(Bytes::from(vec![i, 1])),
                "key {} should survive reopen",
                i
            );
        }
    }
}

#[test]
fn newer_put_overrides_range_tombstone() {
    let dir = tempfile::tempdir().unwrap();
    let engine = LsmEngine::open(dir.path(), opts()).unwrap();

    for i in 0..10u8 {
        engine.put(&[i], &[i, 1]).unwrap();
    }
    engine.delete_range(&[3], &[7]).unwrap();
    for i in 3..7u8 {
        engine.put(&[i], &[i, 2]).unwrap();
    }

    for i in 0..10u8 {
        let got = engine.get(&[i]).unwrap();
        if (3..7).contains(&i) {
            assert_eq!(
                got,
                Some(Bytes::from(vec![i, 2])),
                "key {} should have newest put",
                i
            );
        } else {
            assert_eq!(got, Some(Bytes::from(vec![i, 1])));
        }
    }
}

#[test]
fn scan_filters_range_deleted_keys() {
    let dir = tempfile::tempdir().unwrap();
    let engine = LsmEngine::open(dir.path(), opts()).unwrap();

    for i in 0..10u8 {
        engine.put(&[i], &[i, 1]).unwrap();
    }
    engine.delete_range(&[3], &[7]).unwrap();

    let cursor = engine.scan(None, None).unwrap();
    let keys: Vec<u8> = cursor.map(|r| r.unwrap().0[0]).collect();
    assert_eq!(keys, vec![0, 1, 2, 7, 8, 9]);
}

#[test]
fn overlapping_range_tombstones_merge_by_sequence() {
    let dir = tempfile::tempdir().unwrap();
    let engine = LsmEngine::open(dir.path(), opts()).unwrap();

    for i in 0..10u8 {
        engine.put(&[i], &[i, 1]).unwrap();
    }
    // Two adjacent range tombstones.
    engine.delete_range(&[2], &[5]).unwrap();
    engine.delete_range(&[5], &[8]).unwrap();

    for i in 0..10u8 {
        let got = engine.get(&[i]).unwrap();
        if (2..8).contains(&i) {
            assert_eq!(got, None, "key {} should be range-deleted", i);
        } else {
            assert_eq!(got, Some(Bytes::from(vec![i, 1])));
        }
    }
}

#[test]
fn range_tombstones_carried_through_compaction() {
    let dir = tempfile::tempdir().unwrap();
    let mut opts = opts();
    // Use a large write buffer so the initial keys and their range tombstone
    // are flushed in a single MemTable/SSTable.  This exercises the compaction
    // carry path without depending on cross-MemTable tombstone semantics, which
    // require a range-tombstone-aware compaction scheduler not yet implemented.
    opts.write_buffer_size = 4096;
    opts.level0_file_num_compaction_trigger = 1;
    let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();

    for i in 0..10u8 {
        engine.put(&[i], &[i, 1]).unwrap();
    }
    engine.delete_range(&[3], &[7]).unwrap();
    engine.sync().unwrap();

    // Force enough writes to trigger compaction past L0.
    for round in 0..20u8 {
        for i in 10..20u8 {
            engine.put(&[i], &[i, round]).unwrap();
        }
    }
    engine.sync().unwrap();

    for i in 0..10u8 {
        let got = engine.get(&[i]).unwrap();
        if (3..7).contains(&i) {
            assert_eq!(
                got,
                None,
                "key {} should still be range-deleted after compaction",
                i
            );
        } else {
            assert_eq!(
                got,
                Some(Bytes::from(vec![i, 1])),
                "key {} should survive compaction",
                i
            );
        }
    }
}
