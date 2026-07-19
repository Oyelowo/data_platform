//! `storage_traits::Engine` conformance smoke test for `storage-vector`.
//!
//! Note: because `storage-vector` stores vectors, values must be encoded with
//! `storage_vector::format::encode_f32_vec`. Arbitrary byte values are rejected.

use storage_traits::{Engine, Transaction, TxnOptions};
use storage_vector::{DistanceMetric, VectorEngine, VectorOptions};

fn open() -> (tempfile::TempDir, VectorEngine) {
    let dir = tempfile::tempdir().unwrap();
    let engine = VectorEngine::open(
        dir.path(),
        VectorOptions::brute_force(4, DistanceMetric::Euclidean),
    )
    .unwrap();
    (dir, engine)
}

fn encode(v: &[f32]) -> Vec<u8> {
    storage_vector::format::encode_f32_vec(v)
}

#[test]
fn engine_crud_smoke() {
    let (_dir, engine) = open();
    engine.put(b"a", &[1.0f32, 2.0, 3.0, 4.0]).unwrap();
    assert_eq!(
        <VectorEngine as Engine>::get(&engine, b"a").unwrap(),
        Some(bytes::Bytes::from(encode(&[1.0f32, 2.0, 3.0, 4.0])))
    );
    engine.delete(b"a").unwrap();
    assert!(<VectorEngine as Engine>::get(&engine, b"a").unwrap().is_none());
}

#[test]
fn engine_scan_smoke() {
    let (_dir, engine) = open();
    engine.put(b"a", &[1.0f32; 4]).unwrap();
    engine.put(b"b", &[2.0f32; 4]).unwrap();
    engine.put(b"c", &[3.0f32; 4]).unwrap();
    let mut cursor = <VectorEngine as Engine>::scan(&engine, Some(b"a"), Some(b"c")).unwrap();
    let (k, _) = cursor.next().unwrap().unwrap();
    assert_eq!(k, bytes::Bytes::from_static(b"a"));
    let (k, _) = cursor.next().unwrap().unwrap();
    assert_eq!(k, bytes::Bytes::from_static(b"b"));
    assert!(cursor.next().is_none());
}

#[test]
fn engine_transaction_smoke() {
    let (_dir, engine) = open();
    let mut txn = <VectorEngine as Engine>::begin(&engine, TxnOptions::default()).unwrap();
    txn.put(b"x", &encode(&[5.0f32; 4])).unwrap();
    txn.commit().unwrap();
    assert_eq!(
        <VectorEngine as Engine>::get(&engine, b"x").unwrap(),
        Some(bytes::Bytes::from(encode(&[5.0f32; 4])))
    );
}

#[test]
fn engine_stats_smoke() {
    let (_dir, engine) = open();
    engine.put(b"a", &[1.0f32; 4]).unwrap();
    let stats = <VectorEngine as Engine>::stats(&engine).unwrap();
    assert_eq!(stats.name, "storage-vector");
    assert_eq!(stats.num_keys, Some(1));
}
