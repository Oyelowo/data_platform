//! Integration tests for WiscKey blob garbage collection.

use bytes::Bytes;
use storage_kv::{LsmEngine, LsmOptions};
use storage_traits::{Engine, Transaction};

fn blob_gc_opts() -> LsmOptions {
    LsmOptions {
        // Any value >= 64 bytes is stored in the blob log.
        min_blob_value_size: 64,
        // Rotate blob files after every record so GC can target old files.
        blob_file_size: 1,
        // Tiny MemTable to force flushes and SSTable reads.
        write_buffer_size: 64,
        // Aggressive compaction to exercise blob refs through L1+.
        level0_file_num_compaction_trigger: 2,
        target_file_size_base: 128,
        // GC as soon as a file is less than fully live.
        blob_gc_ratio: 1.0,
        // Disable the background worker; tests drive GC explicitly.
        blob_gc_interval_ms: 0,
        ..Default::default()
    }
}

fn large_value(prefix: u8, len: usize) -> Vec<u8> {
    let mut v = vec![prefix; len];
    if len > 4 {
        v[len - 4..].copy_from_slice(b"tail");
    }
    v
}

fn blob_file_count(dir: &std::path::Path) -> usize {
    let blob_dir = dir.join("blob");
    if !blob_dir.exists() {
        return 0;
    }
    std::fs::read_dir(&blob_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_str()
                .map(|n| n.ends_with(".blob"))
                .unwrap_or(false)
        })
        .count()
}

#[test]
fn gc_reclaims_overwritten_blob_value() {
    let dir = tempfile::tempdir().unwrap();
    let opts = blob_gc_opts();
    let engine = LsmEngine::open(dir.path(), opts).unwrap();

    let key = b"k";
    let v1 = large_value(1, 100);
    let v2 = large_value(2, 100);

    engine.put(key, &v1).unwrap();
    engine.sync().unwrap();
    let blobs_before = blob_file_count(dir.path());
    assert!(blobs_before >= 1);

    engine.put(key, &v2).unwrap();
    engine.sync().unwrap();

    let stats = engine.run_blob_gc_once().unwrap();
    assert!(stats.scanned_files >= 1);
    assert_eq!(stats.deleted_files, 1);

    assert_eq!(engine.get(key).unwrap(), Some(Bytes::from(v2)));
}

#[test]
fn gc_reclaims_deleted_blob_value() {
    let dir = tempfile::tempdir().unwrap();
    let opts = blob_gc_opts();
    let engine = LsmEngine::open(dir.path(), opts).unwrap();

    let key = b"k";
    let value = large_value(1, 100);

    engine.put(key, &value).unwrap();
    engine.sync().unwrap();
    let blobs_before = blob_file_count(dir.path());
    assert!(blobs_before >= 1);

    engine.delete(key).unwrap();
    // A delete does not write a blob, so write another large value to rotate
    // the current blob file and make the old blob file eligible for GC.
    engine.put(b"other", &large_value(3, 100)).unwrap();
    engine.sync().unwrap();

    let stats = engine.run_blob_gc_once().unwrap();
    assert!(stats.scanned_files >= 1);
    assert_eq!(stats.deleted_files, 1);

    assert_eq!(engine.get(key).unwrap(), None);
}

#[test]
fn gc_preserves_blob_value_pinned_by_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let opts = blob_gc_opts();
    let engine = LsmEngine::open(dir.path(), opts).unwrap();

    let key = b"k";
    let v1 = large_value(1, 100);
    let v2 = large_value(2, 100);

    engine.put(key, &v1).unwrap();
    engine.sync().unwrap();

    // Open a read-only transaction that pins the old value.
    let txn = engine.begin(Default::default()).unwrap();
    assert_eq!(txn.get(key).unwrap(), Some(Bytes::from(v1.clone())));

    engine.put(key, &v2).unwrap();
    engine.sync().unwrap();

    // GC must not make the old blob unreadable while the snapshot is alive.
    // Because a snapshot pins the engine view that references the original blob
    // file, deletions are deferred until the snapshot is dropped.
    let stats = engine.run_blob_gc_once().unwrap();
    assert_eq!(engine.get(key).unwrap(), Some(Bytes::from(v2.clone())));
    assert_eq!(stats.deleted_files, 0);

    // The pinned snapshot still sees the old value.
    assert_eq!(txn.get(key).unwrap(), Some(Bytes::from(v1.clone())));

    // After dropping the snapshot, a subsequent GC pass can reclaim obsolete
    // blob files.
    drop(txn);
    let stats = engine.run_blob_gc_once().unwrap();
    assert!(
        stats.deleted_files >= 1,
        "expected at least one obsolete blob file to be reclaimed"
    );
}

#[test]
fn force_threshold_triggers_immediate_background_gc() {
    let dir = tempfile::tempdir().unwrap();
    let mut opts = blob_gc_opts();
    // Enable the background worker with a long interval, but set a low force
    // threshold so overwriting a blob value triggers a back-to-back GC pass.
    opts.blob_gc_interval_ms = 60_000;
    opts.blob_gc_force_threshold = 0.1;
    let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();

    let key = b"k";
    let v1 = large_value(1, 100);
    let v2 = large_value(2, 100);

    engine.put(key, &v1).unwrap();
    engine.sync().unwrap();

    let blobs_before = blob_file_count(dir.path());
    assert!(blobs_before >= 1);

    // Overwrite the value.  The old blob file becomes garbage and the garbage
    // ratio crosses the force threshold, so the worker should run a GC pass
    // without waiting for the 60 second interval.
    engine.put(key, &v2).unwrap();
    engine.sync().unwrap();

    // Poll briefly; the forced pass should reclaim the extra file and return
    // the count to the original level (or below if multiple files were cleaned).
    let mut blobs_after = blob_file_count(dir.path());
    for _ in 0..50 {
        if blobs_after <= blobs_before {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
        blobs_after = blob_file_count(dir.path());
    }

    assert!(
        blobs_after <= blobs_before,
        "forced GC should reclaim the old blob file before the 60s interval fires"
    );
    assert_eq!(engine.get(key).unwrap(), Some(Bytes::from(v2)));
}

#[test]
fn gc_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let opts = blob_gc_opts();
    let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();

    let key = b"k";
    let v1 = large_value(1, 100);
    let v2 = large_value(2, 100);

    engine.put(key, &v1).unwrap();
    engine.put(key, &v2).unwrap();
    engine.sync().unwrap();

    engine.run_blob_gc_once().unwrap();
    assert_eq!(engine.get(key).unwrap(), Some(Bytes::from(v2.clone())));
    drop(engine);

    let engine = LsmEngine::open(dir.path(), opts).unwrap();
    assert_eq!(engine.get(key).unwrap(), Some(Bytes::from(v2)));
}

#[test]
fn compaction_rewrites_blob_ref() {
    let dir = tempfile::tempdir().unwrap();
    let mut opts = blob_gc_opts();
    // Disable standalone GC so any reclaimable file must come from compaction.
    opts.blob_gc_ratio = 1.0;
    opts.blob_gc_interval_ms = 0;
    // Tiny MemTable to force flushes, and aggressive compaction trigger.
    opts.write_buffer_size = 64;
    opts.level0_file_num_compaction_trigger = 2;
    opts.target_file_size_base = 128;
    let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();

    let key = b"k";
    let v1 = large_value(1, 100);
    let v2 = large_value(2, 100);

    engine.put(key, &v1).unwrap();
    engine.sync().unwrap();
    let blobs_before = blob_file_count(dir.path());
    assert!(blobs_before >= 1);

    engine.put(key, &v2).unwrap();
    engine.sync().unwrap();

    // Write enough distinct keys to trigger an L0->L1 compaction.
    for i in 0..20 {
        let k = format!("x{i:03}");
        engine
            .put(k.as_bytes(), &large_value(i as u8, 100))
            .unwrap();
    }
    engine.sync().unwrap();

    // Wait for the background compactor to run.
    std::thread::sleep(std::time::Duration::from_millis(500));

    // The current value must still be readable after compaction rewrote the
    // live BlobRef into the current blob file.
    assert_eq!(engine.get(key).unwrap(), Some(Bytes::from(v2)));
}

#[test]
fn compaction_preserves_snapshot_blob() {
    let dir = tempfile::tempdir().unwrap();
    let mut opts = blob_gc_opts();
    opts.blob_gc_ratio = 1.0;
    opts.blob_gc_interval_ms = 0;
    opts.write_buffer_size = 64;
    opts.level0_file_num_compaction_trigger = 2;
    opts.target_file_size_base = 128;
    let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();

    let key = b"k";
    let v1 = large_value(1, 100);
    let v2 = large_value(2, 100);

    engine.put(key, &v1).unwrap();
    engine.sync().unwrap();

    let txn = engine.begin(Default::default()).unwrap();
    assert_eq!(txn.get(key).unwrap(), Some(Bytes::from(v1.clone())));

    engine.put(key, &v2).unwrap();
    engine.sync().unwrap();

    for i in 0..20 {
        let k = format!("x{i:03}");
        engine
            .put(k.as_bytes(), &large_value(i as u8, 100))
            .unwrap();
    }
    engine.sync().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(500));

    // The pinned snapshot still sees v1; the current view sees v2.
    assert_eq!(txn.get(key).unwrap(), Some(Bytes::from(v1)));
    assert_eq!(engine.get(key).unwrap(), Some(Bytes::from(v2)));
}

fn total_blob_file_size(dir: &std::path::Path) -> u64 {
    let blob_dir = dir.join("blob");
    if !blob_dir.exists() {
        return 0;
    }
    std::fs::read_dir(&blob_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_str()
                .map(|n| n.ends_with(".blob"))
                .unwrap_or(false)
        })
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum()
}

/// Blob accounting must be consistent after reopen (lazy rebuild from disk).
#[test]
fn blob_stats_rebuild_on_open() {
    let dir = tempfile::tempdir().unwrap();
    let mut opts = blob_gc_opts();
    opts.blob_gc_interval_ms = 0;
    let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();

    for i in 0..5 {
        let key = format!("k{i}");
        engine
            .put(key.as_bytes(), &large_value(i, 100))
            .unwrap();
    }
    engine.sync().unwrap();

    let stats_before = engine.blob_stats();
    let disk_before = total_blob_file_size(dir.path());
    assert_eq!(stats_before.total_bytes, disk_before);

    // Run GC to delete any reclaimable files, then reopen.
    engine.run_blob_gc_once().unwrap();
    let stats_after_gc = engine.blob_stats();
    drop(engine);

    let engine = LsmEngine::open(dir.path(), opts).unwrap();
    let stats_after_reopen = engine.blob_stats();
    let disk_after_reopen = total_blob_file_size(dir.path());

    assert_eq!(stats_after_reopen.total_bytes, disk_after_reopen);
    assert_eq!(stats_after_reopen.total_bytes, stats_after_gc.total_bytes);
    assert_eq!(stats_after_reopen.garbage_bytes, 0);
}
