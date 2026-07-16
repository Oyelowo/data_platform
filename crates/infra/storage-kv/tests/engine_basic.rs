//! Basic integration tests for storage-kv.

use storage_kv::{LsmEngine, LsmOptions};
use storage_traits::Engine;

#[test]
fn put_and_get() {
    let dir = tempfile::tempdir().unwrap();
    let opts = LsmOptions {
        write_buffer_size: 256, // tiny to force flushes
        ..Default::default()
    };
    let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();
    engine.put(b"hello", b"world").unwrap();
    assert_eq!(
        engine.get(b"hello").unwrap(),
        Some(bytes::Bytes::from_static(b"world"))
    );
}

#[test]
fn overwrite_and_delete() {
    let dir = tempfile::tempdir().unwrap();
    let opts = LsmOptions {
        write_buffer_size: 256,
        ..Default::default()
    };
    let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();
    engine.put(b"k", b"v1").unwrap();
    engine.put(b"k", b"v2").unwrap();
    assert_eq!(
        engine.get(b"k").unwrap(),
        Some(bytes::Bytes::from_static(b"v2"))
    );
    engine.delete(b"k").unwrap();
    assert_eq!(engine.get(b"k").unwrap(), None);
}

#[test]
fn scan_range() {
    let dir = tempfile::tempdir().unwrap();
    let opts = LsmOptions {
        write_buffer_size: 256,
        ..Default::default()
    };
    let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();
    for i in 0..10u8 {
        engine.put(&[i], &[i + 10]).unwrap();
    }
    let mut cursor = engine.scan(Some(&[2]), Some(&[7])).unwrap();
    let mut results = Vec::new();
    while let Some(Ok((k, v))) = cursor.next() {
        results.push((k[0], v[0]));
    }
    assert_eq!(results, vec![(2, 12), (3, 13), (4, 14), (5, 15), (6, 16)]);
}

#[test]
fn reopen_recovers_data() {
    let dir = tempfile::tempdir().unwrap();
    let opts = LsmOptions {
        write_buffer_size: 256,
        ..Default::default()
    };
    {
        let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();
        for i in 0..50u8 {
            engine.put(&[i], &[i + 100]).unwrap();
        }
        engine.sync().unwrap();
    }

    let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();
    for i in 0..50u8 {
        let expected = vec![i + 100];
        assert_eq!(
            engine.get(&[i]).unwrap(),
            Some(bytes::Bytes::from(expected))
        );
    }
}
