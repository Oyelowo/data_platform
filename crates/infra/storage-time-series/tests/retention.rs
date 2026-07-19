//! TTL retention tests.

use std::time::Duration;

use storage_time_series::{
    CompressionKind, RetentionPolicy, TimeSeriesEngine, TimeSeriesOptions, Value, WalSyncPolicy,
};

fn opts(retention: RetentionPolicy) -> TimeSeriesOptions {
    TimeSeriesOptions {
        retention: Some(retention),
        memtable_size_limit: 64 * 1024,
        chunk_size_target: 1024,
        max_open_chunks: 16,
        compression: CompressionKind::Gorilla,
        wal_sync_policy: WalSyncPolicy::SyncOnEngineSync,
        value_kind: storage_time_series::ValueKind::F64,
        max_key_len: 4096,
        max_chunk_size: 16 * 1024 * 1024,
    }
}

fn series_key() -> Vec<u8> {
    storage_time_series::build_series_key(b"cpu", &[("host".to_string(), "db1".to_string())])
}

#[test]
fn ttl_duration_removes_old_samples() {
    let dir = tempfile::tempdir().unwrap();
    let engine = TimeSeriesEngine::open(
        dir.path(),
        opts(RetentionPolicy::Duration(Duration::from_secs(10))),
    )
    .unwrap();
    let key = series_key();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    for i in 0..100u64 {
        engine
            .put(key.clone(), now - 100 + i, Value::F64(i as f64))
            .unwrap();
    }
    engine.sync().unwrap();
    // Samples older than now - 10s are expired, keeping the newest ~10 seconds.
    let samples = engine.get_range(&key, 0, u64::MAX).unwrap();
    assert!(!samples.is_empty());
    let cutoff = now.saturating_sub(10_000_000_000);
    assert!(
        samples.first().unwrap().timestamp >= cutoff,
        "oldest retained sample {} is before cutoff {}",
        samples.first().unwrap().timestamp,
        cutoff
    );
}

#[test]
fn max_samples_retention_keeps_newest() {
    let dir = tempfile::tempdir().unwrap();
    let engine =
        TimeSeriesEngine::open(dir.path(), opts(RetentionPolicy::MaxSamples(20))).unwrap();
    let key = series_key();
    for i in 0..100u64 {
        engine.put(key.clone(), i, Value::F64(i as f64)).unwrap();
    }
    engine.sync().unwrap();
    let samples = engine.get_range(&key, 0, 100).unwrap();
    assert_eq!(samples.len(), 20);
    assert_eq!(samples.first().unwrap().timestamp, 80);
    assert_eq!(samples.last().unwrap().timestamp, 99);
}
