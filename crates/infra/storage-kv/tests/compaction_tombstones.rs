//! Tombstone behavior across compactions.

use storage_kv::{LsmEngine, LsmOptions};
use storage_traits::Engine;

fn opts() -> LsmOptions {
    LsmOptions {
        write_buffer_size: 64,
        level0_file_num_compaction_trigger: 2,
        target_file_size_base: 256,
        max_bytes_for_level_base: 256,
        ..Default::default()
    }
}

/// A delete must remain effective after compaction, even when older values of
/// the same key exist in lower levels.
#[test]
fn delete_survives_compaction() {
    let dir = tempfile::tempdir().unwrap();
    let opts = opts();
    let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();

    // Write and flush several versions of the same key.
    for round in 0..10u8 {
        engine.put(b"k", &[round]).unwrap();
    }
    engine.delete(b"k").unwrap();
    engine.sync().unwrap();
    drop(engine);

    // Reopen to recover and trigger any background compactions on restart.
    let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();
    for round in 0..10u8 {
        engine.put(b"k", &[round]).unwrap();
    }
    engine.delete(b"k").unwrap();
    engine.sync().unwrap();
    drop(engine);

    let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();
    assert_eq!(engine.get(b"k").unwrap(), None);
}

/// Deleted keys must not reappear in a scan after compaction.
#[test]
fn deleted_keys_do_not_resurrect_after_compaction() {
    let dir = tempfile::tempdir().unwrap();
    let opts = opts();
    let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();

    for round in 0..5u8 {
        for i in 0..50u8 {
            engine.put(&[i], &[round, i]).unwrap();
        }
    }
    // Delete every third key.
    for i in (0..50u8).step_by(3) {
        engine.delete(&[i]).unwrap();
    }
    engine.sync().unwrap();

    // Force more overwrites of the *live* keys to trigger compactions.
    for round in 5..10u8 {
        for i in 0..50u8 {
            if i % 3 != 0 {
                engine.put(&[i], &[round, i]).unwrap();
            }
        }
    }
    engine.sync().unwrap();

    let mut cursor = engine.scan(None, None).unwrap();
    while let Some(Ok((k, _v))) = cursor.next() {
        assert!(k.len() == 1);
        assert!(k[0] % 3 != 0, "deleted key {} resurrected", k[0]);
    }
}
