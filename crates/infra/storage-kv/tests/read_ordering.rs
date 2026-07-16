//! Regression tests for read ordering across the MemTable tiers.
//!
//! Point reads are first-hit-wins across current MemTable, immutable queue
//! (newest to oldest), L0 (newest file first), then levels.  Two historical
//! bugs broke this: the immutable queue was searched oldest-first, and L0
//! file numbers were reserved at flush time so concurrent flushers could
//! invert creation order.  Both allowed an older version to shadow a newer
//! one.

use storage_kv::{LsmEngine, LsmOptions};

/// With a 1-byte write buffer every put freezes the MemTable, so versions of
/// the same key pile up in the immutable queue.  A read must always return
/// the newest version, never one from an older frozen table.
#[test]
fn newest_frozen_memtable_wins() {
    let dir = tempfile::tempdir().unwrap();
    let opts = LsmOptions {
        write_buffer_size: 1, // freeze on every put
        ..Default::default()
    };
    let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();

    for i in 0..200u32 {
        let value = format!("v{i}");
        engine.put(b"k", value.as_bytes()).unwrap();
        assert_eq!(
            engine.get(b"k").unwrap(),
            Some(bytes::Bytes::from(value)),
            "stale read after version {i}"
        );
    }
}

/// The same ordering must hold for many distinct keys, not just one.
#[test]
fn newest_frozen_memtable_wins_across_keys() {
    let dir = tempfile::tempdir().unwrap();
    let opts = LsmOptions {
        write_buffer_size: 1,
        ..Default::default()
    };
    let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();

    for round in 0..50u8 {
        for k in 0..10u8 {
            engine.put(&[k], &[round]).unwrap();
        }
        for k in 0..10u8 {
            assert_eq!(
                engine.get(&[k]).unwrap(),
                Some(bytes::Bytes::from(vec![round])),
                "stale read for key {k} at round {round}"
            );
        }
    }
}

/// Deletes in a newer frozen table must shadow values in an older one.
#[test]
fn delete_in_newer_frozen_table_shadows_older_value() {
    let dir = tempfile::tempdir().unwrap();
    let opts = LsmOptions {
        write_buffer_size: 1,
        ..Default::default()
    };
    let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();

    engine.put(b"k", b"v").unwrap();
    engine.delete(b"k").unwrap();
    assert_eq!(engine.get(b"k").unwrap(), None);

    // And a subsequent put must be visible again.
    engine.put(b"k", b"v2").unwrap();
    assert_eq!(
        engine.get(b"k").unwrap(),
        Some(bytes::Bytes::from_static(b"v2"))
    );
}
