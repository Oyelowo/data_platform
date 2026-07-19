//! Basic CRUD, scan, and transaction tests for `storage-vector`.

use bytes::Bytes;
use storage_traits::{Engine, Transaction, TxnOptions};
use storage_vector::{DistanceMetric, IndexType, VectorEngine, VectorOptions};

fn opts() -> VectorOptions {
    VectorOptions::brute_force(4, DistanceMetric::Euclidean)
}

#[test]
fn put_and_get() {
    let dir = tempfile::tempdir().unwrap();
    let engine = VectorEngine::open(dir.path(), opts()).unwrap();
    engine.put(b"a", &[1.0f32, 2.0, 3.0, 4.0]).unwrap();
    let v = engine.get(b"a").unwrap().unwrap();
    assert_eq!(v, vec![1.0f32, 2.0, 3.0, 4.0]);
}

#[test]
fn engine_get_returns_encoded_vector() {
    let dir = tempfile::tempdir().unwrap();
    let engine = VectorEngine::open(dir.path(), opts()).unwrap();
    engine.put(b"a", &[1.0f32, 2.0, 3.0, 4.0]).unwrap();
    let bytes = engine.get(b"a").unwrap().unwrap();
    assert!(!bytes.is_empty());
}

#[test]
fn dimension_mismatch_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let engine = VectorEngine::open(dir.path(), opts()).unwrap();
    let err = engine.put(b"a", &[1.0f32, 2.0]).unwrap_err();
    assert!(matches!(err, storage_vector::Error::DimensionMismatch { .. }));
}

#[test]
fn delete_removes_vector() {
    let dir = tempfile::tempdir().unwrap();
    let engine = VectorEngine::open(dir.path(), opts()).unwrap();
    engine.put(b"a", &[1.0f32, 2.0, 3.0, 4.0]).unwrap();
    assert!(engine.delete(b"a").unwrap());
    assert!(engine.get(b"a").unwrap().is_none());
    assert!(!engine.delete(b"a").unwrap());
}

#[test]
fn scan_range() {
    let dir = tempfile::tempdir().unwrap();
    let engine = VectorEngine::open(dir.path(), opts()).unwrap();
    engine.put(b"a", &[1.0f32, 0.0, 0.0, 0.0]).unwrap();
    engine.put(b"b", &[2.0f32, 0.0, 0.0, 0.0]).unwrap();
    engine.put(b"c", &[3.0f32, 0.0, 0.0, 0.0]).unwrap();
    let mut cursor = engine.scan(Some(b"a"), Some(b"c")).unwrap();
    let (k, _) = cursor.next().unwrap().unwrap();
    assert_eq!(k, Bytes::from_static(b"a"));
    let (k, _) = cursor.next().unwrap().unwrap();
    assert_eq!(k, Bytes::from_static(b"b"));
    assert!(cursor.next().is_none());
}

#[test]
fn transaction_commit() {
    let dir = tempfile::tempdir().unwrap();
    let engine = VectorEngine::open(dir.path(), opts()).unwrap();
    let mut txn = engine.begin(TxnOptions::default()).unwrap();
    let encoded = storage_vector::format::encode_f32_vec(&[5.0f32, 6.0, 7.0, 8.0]);
    txn.put(b"x", &encoded).unwrap();
    txn.commit().unwrap();
    let v = engine.get(b"x").unwrap().unwrap();
    assert_eq!(v, vec![5.0f32, 6.0, 7.0, 8.0]);
}

#[test]
fn transaction_rollback() {
    let dir = tempfile::tempdir().unwrap();
    let engine = VectorEngine::open(dir.path(), opts()).unwrap();
    let mut txn = engine.begin(TxnOptions::default()).unwrap();
    let encoded = storage_vector::format::encode_f32_vec(&[5.0f32, 6.0, 7.0, 8.0]);
    txn.put(b"x", &encoded).unwrap();
    txn.rollback().unwrap();
    assert!(engine.get(b"x").unwrap().is_none());
}

#[test]
fn transaction_read_only_rejects_writes() {
    let dir = tempfile::tempdir().unwrap();
    let engine = VectorEngine::open(dir.path(), opts()).unwrap();
    let mut txn = engine.begin(TxnOptions::read_only()).unwrap();
    let encoded = storage_vector::format::encode_f32_vec(&[5.0f32, 6.0, 7.0, 8.0]);
    let err = txn.put(b"x", &encoded).unwrap_err();
    assert!(matches!(err, storage_vector::Error::ReadOnlyTransaction));
}

#[test]
fn sync_persists_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    {
        let engine = VectorEngine::open(dir.path(), opts()).unwrap();
        engine.put(b"persisted", &[9.0f32, 8.0, 7.0, 6.0]).unwrap();
        engine.sync().unwrap();
    }
    {
        let engine = VectorEngine::open(dir.path(), opts()).unwrap();
        let v = engine.get(b"persisted").unwrap().unwrap();
        assert_eq!(v, vec![9.0f32, 8.0, 7.0, 6.0]);
    }
}

#[test]
fn stats_report_vector_count() {
    let dir = tempfile::tempdir().unwrap();
    let engine = VectorEngine::open(dir.path(), opts()).unwrap();
    engine.put(b"a", &[1.0f32; 4]).unwrap();
    engine.put(b"b", &[2.0f32; 4]).unwrap();
    let stats = engine.stats().unwrap();
    assert_eq!(stats.num_vectors, 2);
}

#[test]
fn all_index_types_open() {
    for index_type in [IndexType::BruteForce, IndexType::Hnsw, IndexType::Ivf] {
        let dir = tempfile::tempdir().unwrap();
        let opts = VectorOptions {
            index_type,
            dimension: 8,
            metric: DistanceMetric::Euclidean,
            ..VectorOptions::default()
        };
        let engine = VectorEngine::open(dir.path(), opts).unwrap();
        assert_eq!(engine.name(), "storage-vector");
    }
}
