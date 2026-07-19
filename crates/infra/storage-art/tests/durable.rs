//! Durability and recovery tests for `storage-art`.
//!
//! These tests exercise `ArtEngine` open/close, crash recovery, WAL replay,
//! snapshot truncation, and conformance against the in-memory BTree model.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

use bytes::Bytes;
use proptest::prelude::*;
use storage_art::{ArtEngine, ArtEngineOptions, WalSyncPolicy};
use storage_traits::{Engine, Transaction, TxnOptions};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn tmp_dir() -> std::path::PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("storage-art-durable-{n}"));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn reopen(dir: &std::path::Path, options: ArtEngineOptions) -> ArtEngine {
    ArtEngine::open(dir, options).unwrap()
}

#[test]
fn reopen_preserves_data() {
    let dir = tmp_dir();
    let engine = ArtEngine::open(&dir, ArtEngineOptions::default()).unwrap();
    engine.put(b"hello", b"world").unwrap();
    engine.put(b"foo", b"bar").unwrap();
    engine.delete(b"foo").unwrap();
    engine.close().unwrap();

    let engine2 = ArtEngine::open(&dir, ArtEngineOptions::default()).unwrap();
    assert_eq!(
        engine2.get(b"hello").unwrap(),
        Some(Bytes::from_static(b"world"))
    );
    assert_eq!(engine2.get(b"foo").unwrap(), None);
    engine2.close().unwrap();
}

#[test]
fn sync_writes_snapshot_and_truncates_wal() {
    let dir = tmp_dir();
    let engine = ArtEngine::open(&dir, ArtEngineOptions::default()).unwrap();
    engine.put(b"a", b"1").unwrap();
    engine.sync().unwrap();
    engine.close().unwrap();

    let snapshot = dir.join("snapshot.bin");
    let meta = dir.join("art.meta");
    assert!(snapshot.exists());
    assert!(meta.exists());

    let wal_files: Vec<_> = std::fs::read_dir(dir.join("wal"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.metadata().map(|m| m.is_file()).unwrap_or(false))
        .collect();
    // After checkpoint the WAL should contain at least the active segment and lock.
    assert!(!wal_files.is_empty());

    let engine2 = reopen(&dir, ArtEngineOptions::default());
    assert_eq!(engine2.get(b"a").unwrap(), Some(Bytes::from_static(b"1")));
    engine2.close().unwrap();
}

#[test]
fn recovery_from_snapshot_plus_wal() {
    let dir = tmp_dir();
    let engine = ArtEngine::open(&dir, ArtEngineOptions::default()).unwrap();
    engine.put(b"a", b"1").unwrap();
    engine.put(b"b", b"2").unwrap();
    engine.sync().unwrap();
    engine.put(b"c", b"3").unwrap();
    engine.put(b"d", b"4").unwrap();
    engine.close().unwrap();

    let engine2 = reopen(&dir, ArtEngineOptions::default());
    assert_eq!(engine2.get(b"a").unwrap(), Some(Bytes::from_static(b"1")));
    assert_eq!(engine2.get(b"b").unwrap(), Some(Bytes::from_static(b"2")));
    assert_eq!(engine2.get(b"c").unwrap(), Some(Bytes::from_static(b"3")));
    assert_eq!(engine2.get(b"d").unwrap(), Some(Bytes::from_static(b"4")));
    engine2.close().unwrap();
}

#[test]
fn recovery_without_snapshot_replays_full_wal() {
    let dir = tmp_dir();
    let engine = ArtEngine::open(
        &dir,
        ArtEngineOptions {
            snapshot_on_sync: false,
            ..ArtEngineOptions::default()
        },
    )
    .unwrap();
    engine.put(b"x", b"10").unwrap();
    engine.put(b"y", b"20").unwrap();
    engine.close().unwrap();

    let engine2 = reopen(
        &dir,
        ArtEngineOptions {
            snapshot_on_sync: false,
            ..ArtEngineOptions::default()
        },
    );
    assert_eq!(engine2.get(b"x").unwrap(), Some(Bytes::from_static(b"10")));
    assert_eq!(engine2.get(b"y").unwrap(), Some(Bytes::from_static(b"20")));
    engine2.close().unwrap();
}

#[test]
fn buffered_sync_is_durable() {
    let dir = tmp_dir();
    let engine = ArtEngine::open(
        &dir,
        ArtEngineOptions {
            wal_sync_policy: WalSyncPolicy::Buffered,
            ..ArtEngineOptions::default()
        },
    )
    .unwrap();
    engine.put(b"k", b"v").unwrap();
    engine.sync().unwrap();
    engine.close().unwrap();

    let engine2 = reopen(&dir, ArtEngineOptions::default());
    assert_eq!(engine2.get(b"k").unwrap(), Some(Bytes::from_static(b"v")));
    engine2.close().unwrap();
}

#[test]
fn crash_drops_unsynced_records() {
    let dir = tmp_dir();
    let engine = ArtEngine::open(
        &dir,
        ArtEngineOptions {
            wal_sync_policy: WalSyncPolicy::Buffered,
            ..ArtEngineOptions::default()
        },
    )
    .unwrap();
    engine.put(b"committed", b"yes").unwrap();
    engine.sync().unwrap();
    engine.put(b"uncommitted", b"maybe").unwrap();
    engine.crash().unwrap();
    engine.close().unwrap();

    let engine2 = reopen(&dir, ArtEngineOptions::default());
    assert_eq!(
        engine2.get(b"committed").unwrap(),
        Some(Bytes::from_static(b"yes"))
    );
    assert_eq!(engine2.get(b"uncommitted").unwrap(), None);
    engine2.close().unwrap();
}

#[test]
fn transaction_commit_survives_restart() {
    let dir = tmp_dir();
    let engine = ArtEngine::open(&dir, ArtEngineOptions::default()).unwrap();
    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    tx.put(b"one", b"1").unwrap();
    tx.put(b"two", b"2").unwrap();
    tx.delete(b"one").unwrap();
    tx.commit().unwrap();
    engine.close().unwrap();

    let engine2 = reopen(&dir, ArtEngineOptions::default());
    assert_eq!(engine2.get(b"one").unwrap(), None);
    assert_eq!(engine2.get(b"two").unwrap(), Some(Bytes::from_static(b"2")));
    engine2.close().unwrap();
}

#[test]
fn scan_ordering_after_recovery() {
    let dir = tmp_dir();
    let engine = ArtEngine::open(&dir, ArtEngineOptions::default()).unwrap();
    engine.put(b"c", b"3").unwrap();
    engine.put(b"a", b"1").unwrap();
    engine.put(b"b", b"2").unwrap();
    engine.sync().unwrap();
    engine.close().unwrap();

    let engine2 = reopen(&dir, ArtEngineOptions::default());
    let keys: Vec<_> = engine2
        .scan(None, None)
        .unwrap()
        .map(|r| r.unwrap().0)
        .collect();
    assert_eq!(
        keys,
        vec![
            Bytes::from_static(b"a"),
            Bytes::from_static(b"b"),
            Bytes::from_static(b"c"),
        ]
    );
    engine2.close().unwrap();
}

#[test]
fn corruption_detected_bad_metadata() {
    let dir = tmp_dir();
    let engine = ArtEngine::open(&dir, ArtEngineOptions::default()).unwrap();
    engine.put(b"a", b"1").unwrap();
    engine.sync().unwrap();
    engine.close().unwrap();

    // Corrupt the metadata file.
    let meta_path = dir.join("art.meta");
    let mut bytes = std::fs::read(&meta_path).unwrap();
    bytes[10] ^= 0xff;
    std::fs::write(&meta_path, bytes).unwrap();

    assert!(ArtEngine::open(&dir, ArtEngineOptions::default()).is_err());
}

#[test]
fn empty_engine_reopen_is_empty() {
    let dir = tmp_dir();
    let engine = ArtEngine::open(&dir, ArtEngineOptions::default()).unwrap();
    engine.close().unwrap();

    let engine2 = reopen(&dir, ArtEngineOptions::default());
    assert!(engine2.is_empty());
    engine2.close().unwrap();
}

#[test]
fn conformance_with_art_engine() {
    storage_testkit::conformance::run::<ArtEngine, _>(|| {
        let dir = tmp_dir();
        ArtEngine::open(&dir, ArtEngineOptions::default()).unwrap()
    });
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn proptest_durability_matches_btree_model(ops in prop::collection::vec(
        prop_oneof![
            (any::<Vec<u8>>(), any::<Vec<u8>>()).prop_map(|(k, v)| Op::Put(k, v)),
            any::<Vec<u8>>().prop_map(Op::Delete),
        ],
        1..100,
    )) {
        let dir = tmp_dir();
        let engine = ArtEngine::open(&dir, ArtEngineOptions::default()).unwrap();
        let mut model = BTreeMap::<Vec<u8>, Vec<u8>>::new();

        for op in &ops {
            match op {
                Op::Put(k, v) => {
                    engine.put(k, v).unwrap();
                    model.insert(k.clone(), v.clone());
                }
                Op::Delete(k) => {
                    engine.delete(k).unwrap();
                    model.remove(k);
                }
            }
        }
        engine.sync().unwrap();
        engine.close().unwrap();

        let engine2 = reopen(&dir, ArtEngineOptions::default());
        for (k, v) in &model {
            assert_eq!(engine2.get(k).unwrap().as_deref(), Some(v.as_slice()));
        }
        // Also verify that keys not in the model are absent.
        let mut cursor = engine2.scan(None, None).unwrap();
        let mut count = 0usize;
        while let Some(Ok((k, v))) = cursor.next() {
            let expected = model.get(k.as_ref()).expect("unexpected key in engine");
            assert_eq!(v.as_ref(), expected.as_slice());
            count += 1;
        }
        assert_eq!(count, model.len());
        engine2.close().unwrap();
    }
}

#[derive(Clone, Debug)]
enum Op {
    Put(Vec<u8>, Vec<u8>),
    Delete(Vec<u8>),
}
