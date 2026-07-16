//! Streaming cursor integration tests for storage-kv.
//!
//! These exercises exercise the `Cursor` trait over the LSM engine: batched
//! streaming, range bounds, snapshot isolation, deletion filtering, and seek.

use storage_kv::{LsmEngine, LsmOptions};
use storage_traits::{Cursor, Engine, Transaction, TxnOptions};

fn small_buffer_opts() -> LsmOptions {
    LsmOptions {
        write_buffer_size: 256,
        ..Default::default()
    }
}

fn key_byte(k: u8) -> bytes::Bytes {
    bytes::Bytes::copy_from_slice(&[k])
}

fn val_byte(v: u8) -> bytes::Bytes {
    bytes::Bytes::copy_from_slice(&[v])
}

#[test]
fn stream_batches_over_memtable_and_sstable() {
    let dir = tempfile::tempdir().unwrap();
    let engine = LsmEngine::open(dir.path(), small_buffer_opts()).unwrap();

    // Force some keys into SSTables.
    for i in 0..5u8 {
        engine.put(&[i], &[i + 100]).unwrap();
    }
    engine.sync().unwrap();

    // Leave additional keys in the mutable MemTable.
    for i in 10..15u8 {
        engine.put(&[i], &[i + 100]).unwrap();
    }

    let mut cursor = engine.scan(None, None).unwrap();
    let first = cursor.next_batch(4).unwrap();
    assert_eq!(
        first,
        (0..4)
            .map(|i| (key_byte(i), val_byte(i + 100)))
            .collect::<Vec<_>>()
    );

    let second = cursor.next_batch(10).unwrap();
    assert_eq!(
        second,
        (4..5)
            .chain(10..15)
            .map(|i| (key_byte(i), val_byte(i + 100)))
            .collect::<Vec<_>>()
    );

    assert!(cursor.next_batch(1).unwrap().is_empty());
}

#[test]
fn range_boundaries_are_exclusive_at_end() {
    let dir = tempfile::tempdir().unwrap();
    let engine = LsmEngine::open(dir.path(), small_buffer_opts()).unwrap();
    for i in 0..10u8 {
        engine.put(&[i], &[i + 10]).unwrap();
    }

    let mut cursor = engine.scan(Some(&[2]), Some(&[7])).unwrap();
    let mut got = Vec::new();
    while let Some(Ok((k, v))) = cursor.next() {
        got.push((k[0], v[0]));
    }
    assert_eq!(got, vec![(2, 12), (3, 13), (4, 14), (5, 15), (6, 16)]);
}

#[test]
fn snapshot_hides_later_writes() {
    let dir = tempfile::tempdir().unwrap();
    let engine = LsmEngine::open(dir.path(), small_buffer_opts()).unwrap();
    for i in 0..5u8 {
        engine.put(&[i], &[i + 1]).unwrap();
    }

    let txn = engine.begin(TxnOptions::read_only()).unwrap();

    // These writes happen after the snapshot was acquired.
    for i in 10..15u8 {
        engine.put(&[i], &[i + 1]).unwrap();
    }

    let mut cursor = txn.scan(None, None).unwrap();
    let mut got = Vec::new();
    while let Some(Ok((k, v))) = cursor.next() {
        got.push((k[0], v[0]));
    }
    assert_eq!(got, vec![(0, 1), (1, 2), (2, 3), (3, 4), (4, 5)]);
}

#[test]
fn deletions_are_filtered_from_scan() {
    let dir = tempfile::tempdir().unwrap();
    let engine = LsmEngine::open(dir.path(), small_buffer_opts()).unwrap();
    for i in 0..5u8 {
        engine.put(&[i], &[i + 1]).unwrap();
    }
    engine.delete(&[2]).unwrap();

    let mut cursor = engine.scan(None, None).unwrap();
    let mut got = Vec::new();
    while let Some(Ok((k, v))) = cursor.next() {
        got.push(k[0]);
        assert_eq!(v[0], k[0] + 1);
    }
    assert_eq!(got, vec![0, 1, 3, 4]);
}

#[test]
fn seek_repositions_cursor() {
    let dir = tempfile::tempdir().unwrap();
    let engine = LsmEngine::open(dir.path(), small_buffer_opts()).unwrap();
    for i in 0..10u8 {
        engine.put(&[i * 2], &[i + 10]).unwrap();
    }

    let mut cursor = engine.scan(None, None).unwrap();
    cursor.seek(&[6]).unwrap();
    let mut got = Vec::new();
    while let Some(Ok((k, v))) = cursor.next() {
        got.push((k[0], v[0]));
    }
    assert_eq!(
        got,
        vec![
            (6, 13),
            (8, 14),
            (10, 15),
            (12, 16),
            (14, 17),
            (16, 18),
            (18, 19)
        ]
    );
}
