//! Edge-case integration tests for storage-kv.

use storage_kv::{LsmEngine, LsmOptions};
use storage_traits::{Cursor, Engine};

fn tiny_block_opts() -> LsmOptions {
    LsmOptions {
        write_buffer_size: 1024,
        block_size: 256,
        ..Default::default()
    }
}

#[test]
fn empty_key_and_value_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let engine = LsmEngine::open(dir.path(), tiny_block_opts()).unwrap();
    engine.put(b"", b"").unwrap();
    assert_eq!(
        engine.get(b"").unwrap(),
        Some(bytes::Bytes::from_static(b""))
    );
}

#[test]
fn large_value_spans_multiple_blocks() {
    let dir = tempfile::tempdir().unwrap();
    let engine = LsmEngine::open(dir.path(), tiny_block_opts()).unwrap();
    let value = vec![0xABu8; 8 * 1024];
    engine.put(b"big", &value).unwrap();
    engine.sync().unwrap();

    assert_eq!(
        engine.get(b"big").unwrap(),
        Some(bytes::Bytes::from(value.clone()))
    );

    let mut cursor = engine.scan(None, None).unwrap();
    let (k, v) = cursor.next().unwrap().unwrap();
    assert_eq!(k, bytes::Bytes::from_static(b"big"));
    assert_eq!(v, bytes::Bytes::from(value));
    assert!(cursor.next().is_none());
}

#[test]
fn many_versions_of_same_key() {
    let dir = tempfile::tempdir().unwrap();
    let engine = LsmEngine::open(dir.path(), tiny_block_opts()).unwrap();
    for i in 0..10u8 {
        engine.put(b"k", &[i]).unwrap();
        engine.sync().unwrap();
    }

    assert_eq!(
        engine.get(b"k").unwrap(),
        Some(bytes::Bytes::from_static(&[9]))
    );

    let mut cursor = engine.scan(None, None).unwrap();
    let (k, v) = cursor.next().unwrap().unwrap();
    assert_eq!(k, bytes::Bytes::from_static(b"k"));
    assert_eq!(v, bytes::Bytes::from_static(&[9]));
    assert!(cursor.next().is_none());
}

#[test]
fn scan_fully_deleted_range() {
    let dir = tempfile::tempdir().unwrap();
    let engine = LsmEngine::open(dir.path(), tiny_block_opts()).unwrap();
    for i in 0..5u8 {
        engine.put(&[i], &[i + 10]).unwrap();
    }
    engine.sync().unwrap();
    for i in 0..5u8 {
        engine.delete(&[i]).unwrap();
    }

    let mut cursor = engine.scan(None, None).unwrap();
    assert!(cursor.next().is_none());
}

#[test]
fn seek_past_all_keys_exhausts_cursor() {
    let dir = tempfile::tempdir().unwrap();
    let engine = LsmEngine::open(dir.path(), tiny_block_opts()).unwrap();
    for i in 0..5u8 {
        engine.put(&[i], &[i + 10]).unwrap();
    }

    let mut cursor = engine.scan(None, None).unwrap();
    cursor.seek(&[9]).unwrap();
    assert!(cursor.next().is_none());
}

#[test]
fn reopen_after_compaction_preserves_visible_state() {
    let dir = tempfile::tempdir().unwrap();
    let engine = LsmEngine::open(dir.path(), tiny_block_opts()).unwrap();
    for i in 0..50u8 {
        engine.put(&[i], &[i + 100]).unwrap();
    }
    engine.sync().unwrap();

    // Overwrite half the keys to create older versions that compaction can drop.
    for i in 0..25u8 {
        engine.put(&[i], &[i + 200]).unwrap();
    }
    engine.sync().unwrap();

    drop(engine);
    let engine = LsmEngine::open(dir.path(), tiny_block_opts()).unwrap();
    for i in 0..25u8 {
        assert_eq!(
            engine.get(&[i]).unwrap(),
            Some(bytes::Bytes::copy_from_slice(&[i + 200]))
        );
    }
    for i in 25..50u8 {
        assert_eq!(
            engine.get(&[i]).unwrap(),
            Some(bytes::Bytes::copy_from_slice(&[i + 100]))
        );
    }
}
