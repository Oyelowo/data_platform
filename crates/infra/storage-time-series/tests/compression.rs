//! Chunk compression round-trip tests.

use storage_time_series::{
    CompressionKind, TimeSeriesEngine, TimeSeriesOptions, Value, WalSyncPolicy,
};

fn opts(compression: CompressionKind) -> TimeSeriesOptions {
    TimeSeriesOptions {
        compression,
        memtable_size_limit: 64 * 1024,
        chunk_size_target: 1024,
        max_open_chunks: 16,
        wal_sync_policy: WalSyncPolicy::SyncOnEngineSync,
        value_kind: storage_time_series::ValueKind::F64,
        max_key_len: 4096,
        max_chunk_size: 16 * 1024 * 1024,
        retention: None,
    }
}

fn series_key(host: &str) -> Vec<u8> {
    storage_time_series::build_series_key(b"cpu", &[("host".to_string(), host.to_string())])
}

#[test]
fn gorilla_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let engine = TimeSeriesEngine::open(dir.path(), opts(CompressionKind::Gorilla)).unwrap();
    let key = series_key("db1");
    for i in 0..200u64 {
        engine.put(key.clone(), i, Value::F64(i as f64)).unwrap();
    }
    engine.sync().unwrap();
    let samples = engine.get_range(&key, 0, 200).unwrap();
    assert_eq!(samples.len(), 200);
    for (i, s) in samples.iter().enumerate() {
        assert_eq!(s.timestamp, i as u64);
        assert_eq!(s.value, Value::F64(i as f64));
    }
}

#[test]
fn none_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let engine = TimeSeriesEngine::open(dir.path(), opts(CompressionKind::None)).unwrap();
    let key = series_key("db1");
    for i in 0..50u64 {
        engine.put(key.clone(), i, Value::F64((i * i) as f64)).unwrap();
    }
    engine.sync().unwrap();
    let samples = engine.get_range(&key, 0, 50).unwrap();
    assert_eq!(samples.len(), 50);
}

#[test]
fn zstd_bytes_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let engine = TimeSeriesEngine::open(dir.path(), opts(CompressionKind::Zstd)).unwrap();
    let key = series_key("db1");
    for i in 0..50u64 {
        engine
            .put(
                key.clone(),
                i,
                Value::Bytes(format!("payload-{i}").into_bytes()),
            )
            .unwrap();
    }
    engine.sync().unwrap();
    let samples = engine.get_range(&key, 0, 50).unwrap();
    assert_eq!(samples.len(), 50);
    for (i, s) in samples.iter().enumerate() {
        assert_eq!(
            s.value,
            Value::Bytes(format!("payload-{i}").into_bytes())
        );
    }
}
