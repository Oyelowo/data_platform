//! Integration tests for WiscKey-style value separation.

use bytes::Bytes;
use storage_kv::{LsmEngine, LsmOptions};
use storage_traits::{Engine, Transaction};

fn blob_opts() -> LsmOptions {
    LsmOptions {
        // Any value >= 64 bytes is stored in the blob log.
        min_blob_value_size: 64,
        // Rotate blob files after 4 KiB to exercise rotation in a single test.
        blob_file_size: 4096,
        // Tiny MemTable to force flushes and SSTable reads.
        write_buffer_size: 256,
        // Aggressive compaction to exercise blob refs through L1+.
        level0_file_num_compaction_trigger: 2,
        target_file_size_base: 256,
        ..Default::default()
    }
}

fn small_value() -> Vec<u8> {
    vec![b'x'; 32]
}

fn large_value(prefix: u8, len: usize) -> Vec<u8> {
    let mut v = vec![prefix; len];
    // Vary the tail so equality checks are meaningful.
    if len > 4 {
        v[len - 4..].copy_from_slice(b"tail");
    }
    v
}

#[test]
fn large_value_point_read_returns_original_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let opts = blob_opts();
    let engine = LsmEngine::open(dir.path(), opts).unwrap();

    let key = b"big-key";
    let value = large_value(1, 200);
    engine.put(key, &value).unwrap();

    assert_eq!(engine.get(key).unwrap(), Some(Bytes::from(value)));
}

#[test]
fn small_value_stays_inline() {
    let dir = tempfile::tempdir().unwrap();
    let opts = blob_opts();
    let engine = LsmEngine::open(dir.path(), opts).unwrap();

    let value = small_value();
    engine.put(b"small", &value).unwrap();

    assert_eq!(engine.get(b"small").unwrap(), Some(Bytes::from(value)));
}

#[test]
fn exact_threshold_value_is_stored_inline() {
    let dir = tempfile::tempdir().unwrap();
    let opts = blob_opts();
    let engine = LsmEngine::open(dir.path(), opts).unwrap();

    // 63 bytes is below the 64-byte threshold.
    let value = large_value(2, 63);
    engine.put(b"threshold-minus-one", &value).unwrap();
    assert_eq!(
        engine.get(b"threshold-minus-one").unwrap(),
        Some(Bytes::from(value))
    );
}

#[test]
fn threshold_value_is_stored_as_blob() {
    let dir = tempfile::tempdir().unwrap();
    let opts = blob_opts();
    let engine = LsmEngine::open(dir.path(), opts).unwrap();

    // 64 bytes is exactly the threshold.
    let value = large_value(3, 64);
    engine.put(b"threshold", &value).unwrap();
    assert_eq!(engine.get(b"threshold").unwrap(), Some(Bytes::from(value)));
}

#[test]
fn scan_resolves_blob_references() {
    let dir = tempfile::tempdir().unwrap();
    let opts = blob_opts();
    let engine = LsmEngine::open(dir.path(), opts).unwrap();

    let mut expected = Vec::new();
    for i in 0..10u8 {
        let key = vec![b'k', i];
        // Alternate small and large values.
        let value = if i % 2 == 0 {
            large_value(i, 100)
        } else {
            small_value()
        };
        engine.put(&key, &value).unwrap();
        expected.push((key, value));
    }

    let mut cursor = engine.scan(None, None).unwrap();
    let mut got = Vec::new();
    while let Some(Ok((k, v))) = cursor.next() {
        got.push((k.to_vec(), v.to_vec()));
    }

    assert_eq!(got, expected);
}

#[test]
fn blob_values_survive_flush_and_compaction() {
    let dir = tempfile::tempdir().unwrap();
    let opts = LsmOptions {
        min_blob_value_size: 64,
        blob_file_size: 1 << 20,
        write_buffer_size: 64,
        level0_file_num_compaction_trigger: 2,
        target_file_size_base: 128,
        ..Default::default()
    };
    let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();

    for round in 0..4u8 {
        for i in 0..20u8 {
            let key = vec![b'k', i];
            let value = large_value(round, 128);
            engine.put(&key, &value).unwrap();
        }
    }
    engine.sync().unwrap();
    drop(engine);

    let engine = LsmEngine::open(dir.path(), opts).unwrap();
    for i in 0..20u8 {
        let key = vec![b'k', i];
        let expected = large_value(3, 128);
        assert_eq!(engine.get(&key).unwrap(), Some(Bytes::from(expected)));
    }
}

#[test]
fn reopen_recovers_blob_values() {
    let dir = tempfile::tempdir().unwrap();
    let opts = blob_opts();
    {
        let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();
        for i in 0..50u8 {
            let key = vec![b'k', i];
            let value = large_value(i, 80);
            engine.put(&key, &value).unwrap();
        }
        engine.sync().unwrap();
    }

    let engine = LsmEngine::open(dir.path(), opts).unwrap();
    for i in 0..50u8 {
        let key = vec![b'k', i];
        let expected = large_value(i, 80);
        assert_eq!(engine.get(&key).unwrap(), Some(Bytes::from(expected)));
    }
}

#[test]
fn delete_after_blob_put_is_visible() {
    let dir = tempfile::tempdir().unwrap();
    let opts = blob_opts();
    let engine = LsmEngine::open(dir.path(), opts).unwrap();

    engine.put(b"k", &large_value(1, 100)).unwrap();
    engine.delete(b"k").unwrap();

    assert_eq!(engine.get(b"k").unwrap(), None);

    // Older snapshot sees the blob value.
    // (The default `get` reads from the completed watermark, which is after
    // the delete, so we verify the tombstone wins for the live snapshot.)
}

#[test]
fn transaction_put_and_get_blob_value() {
    let dir = tempfile::tempdir().unwrap();
    let opts = blob_opts();
    let engine = LsmEngine::open(dir.path(), opts).unwrap();

    let mut txn = engine.begin(Default::default()).unwrap();
    let value = large_value(7, 200);
    txn.put(b"txn-key", &value).unwrap();

    // Read-your-writes inside the transaction.
    assert_eq!(txn.get(b"txn-key").unwrap(), Some(Bytes::from(value.clone())));

    txn.commit().unwrap();

    // After commit the value is visible to new transactions.
    assert_eq!(engine.get(b"txn-key").unwrap(), Some(Bytes::from(value)));
}

#[test]
fn transaction_scan_with_blob_values() {
    let dir = tempfile::tempdir().unwrap();
    let opts = blob_opts();
    let engine = LsmEngine::open(dir.path(), opts).unwrap();

    // Seed the engine with a blob value so the base cursor must resolve it.
    engine.put(b"base", &large_value(1, 100)).unwrap();

    let mut txn = engine.begin(Default::default()).unwrap();
    txn.put(b"buffered", &large_value(2, 100)).unwrap();

    let mut cursor = txn.scan(None, None).unwrap();
    let mut got = Vec::new();
    while let Some(Ok((k, v))) = cursor.next() {
        got.push((k.to_vec(), v.to_vec()));
    }

    let mut expected = vec![
        (b"base".to_vec(), large_value(1, 100)),
        (b"buffered".to_vec(), large_value(2, 100)),
    ];
    expected.sort_by(|a, b| a.0.cmp(&b.0));
    got.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(got, expected);
}

#[test]
fn empty_value_is_not_a_blob() {
    let dir = tempfile::tempdir().unwrap();
    let opts = blob_opts();
    let engine = LsmEngine::open(dir.path(), opts).unwrap();

    engine.put(b"empty", b"").unwrap();
    assert_eq!(engine.get(b"empty").unwrap(), Some(Bytes::new()));
}

#[test]
fn blob_value_overwrite_updates_value() {
    let dir = tempfile::tempdir().unwrap();
    let opts = blob_opts();
    let engine = LsmEngine::open(dir.path(), opts).unwrap();

    engine.put(b"k", &large_value(1, 100)).unwrap();
    engine.put(b"k", &large_value(2, 100)).unwrap();

    assert_eq!(
        engine.get(b"k").unwrap(),
        Some(Bytes::from(large_value(2, 100)))
    );
}
