//! Basic CRUD, scan, and transaction tests for `storage-time-series`.

use storage_traits::{Engine, Transaction, TxnOptions};
use storage_time_series::{
    CompressionKind, TimeSeriesEngine, TimeSeriesOptions, Value, WalSyncPolicy,
};

fn opts() -> TimeSeriesOptions {
    TimeSeriesOptions {
        memtable_size_limit: 64 * 1024,
        chunk_size_target: 1024,
        max_open_chunks: 16,
        compression: CompressionKind::Gorilla,
        wal_sync_policy: WalSyncPolicy::SyncOnEngineSync,
        value_kind: storage_time_series::ValueKind::F64,
        max_key_len: 4096,
        max_chunk_size: 16 * 1024 * 1024,
        retention: None,
    }
}

fn series_key(metric: &[u8], host: &str) -> Vec<u8> {
    storage_time_series::build_series_key(metric, &[("host".to_string(), host.to_string())])
}

#[test]
fn put_and_get_latest() {
    let dir = tempfile::tempdir().unwrap();
    let engine = TimeSeriesEngine::open(dir.path(), opts()).unwrap();
    let key = series_key(b"cpu", "db1");
    engine.put(key.clone(), 100, Value::F64(0.5)).unwrap();
    let latest = engine.get_latest(&key).unwrap().unwrap();
    assert_eq!(latest.timestamp, 100);
    assert_eq!(latest.value, Value::F64(0.5));
}

#[test]
fn get_range_merges_memtable_and_chunks() {
    let dir = tempfile::tempdir().unwrap();
    let engine = TimeSeriesEngine::open(dir.path(), opts()).unwrap();
    let key = series_key(b"cpu", "db1");
    for i in 0..100u64 {
        engine.put(key.clone(), i, Value::F64(i as f64)).unwrap();
    }
    engine.sync().unwrap();
    for i in 100..150u64 {
        engine.put(key.clone(), i, Value::F64(i as f64)).unwrap();
    }
    let samples = engine.get_range(&key, 50, 120).unwrap();
    assert_eq!(samples.len(), 70);
    assert_eq!(samples.first().unwrap().timestamp, 50);
    assert_eq!(samples.last().unwrap().timestamp, 119);
}

#[test]
fn engine_get_returns_encoded_value() {
    let dir = tempfile::tempdir().unwrap();
    let engine = TimeSeriesEngine::open(dir.path(), opts()).unwrap();
    let key = series_key(b"cpu", "db1");
    engine.put(key.clone(), 100, Value::F64(0.5)).unwrap();
    let composite = storage_time_series::format::encode_composite_key(&key, 100);
    let bytes = engine.get(&composite).unwrap().unwrap();
    assert!(!bytes.is_empty());
}

#[test]
fn engine_scan_range() {
    let dir = tempfile::tempdir().unwrap();
    let engine = TimeSeriesEngine::open(dir.path(), opts()).unwrap();
    let key = series_key(b"cpu", "db1");
    engine.put(key.clone(), 1, Value::F64(1.0)).unwrap();
    engine.put(key.clone(), 2, Value::F64(2.0)).unwrap();
    engine.put(key.clone(), 3, Value::F64(3.0)).unwrap();
    let start = storage_time_series::format::encode_composite_key(&key, 1);
    let end = storage_time_series::format::encode_composite_key(&key, 3);
    let count = engine.scan(Some(&start), Some(&end)).unwrap().count();
    assert_eq!(count, 2);
}

#[test]
fn transaction_commit() {
    let dir = tempfile::tempdir().unwrap();
    let engine = TimeSeriesEngine::open(dir.path(), opts()).unwrap();
    let mut txn = engine.begin(TxnOptions::default()).unwrap();
    let key = series_key(b"cpu", "db1");
    txn.put(
        &storage_time_series::format::encode_composite_key(&key, 10),
        &Value::F64(9.0).encode(),
    )
    .unwrap();
    txn.commit().unwrap();
    let latest = engine.get_latest(&key).unwrap().unwrap();
    assert_eq!(latest.value, Value::F64(9.0));
}

#[test]
fn transaction_rollback() {
    let dir = tempfile::tempdir().unwrap();
    let engine = TimeSeriesEngine::open(dir.path(), opts()).unwrap();
    let mut txn = engine.begin(TxnOptions::default()).unwrap();
    let key = series_key(b"cpu", "db1");
    txn.put(
        &storage_time_series::format::encode_composite_key(&key, 10),
        &Value::F64(9.0).encode(),
    )
    .unwrap();
    txn.rollback().unwrap();
    assert!(engine.get_latest(&key).unwrap().is_none());
}

#[test]
fn transaction_read_only_rejects_writes() {
    let dir = tempfile::tempdir().unwrap();
    let engine = TimeSeriesEngine::open(dir.path(), opts()).unwrap();
    let mut txn = engine.begin(TxnOptions::read_only()).unwrap();
    let key = series_key(b"cpu", "db1");
    let err = txn
        .put(
            &storage_time_series::format::encode_composite_key(&key, 10),
            &Value::F64(9.0).encode(),
        )
        .unwrap_err();
    assert!(matches!(err, storage_time_series::Error::ReadOnlyTransaction));
}

#[test]
fn sync_persists_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let key = series_key(b"cpu", "db1");
    {
        let engine = TimeSeriesEngine::open(dir.path(), opts()).unwrap();
        for i in 0..200u64 {
            engine.put(key.clone(), i, Value::F64(i as f64)).unwrap();
        }
        engine.sync().unwrap();
    }
    {
        let engine = TimeSeriesEngine::open(dir.path(), opts()).unwrap();
        let latest = engine.get_latest(&key).unwrap().unwrap();
        assert_eq!(latest.timestamp, 199);
    }
}

#[test]
fn stats_report_series_count() {
    let dir = tempfile::tempdir().unwrap();
    let engine = TimeSeriesEngine::open(dir.path(), opts()).unwrap();
    engine
        .put(series_key(b"cpu", "a"), 1, Value::F64(1.0))
        .unwrap();
    engine
        .put(series_key(b"cpu", "b"), 1, Value::F64(2.0))
        .unwrap();
    let stats = engine.stats().unwrap();
    assert_eq!(stats.num_series, 2);
}
