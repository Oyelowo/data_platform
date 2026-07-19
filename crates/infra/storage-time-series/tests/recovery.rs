//! Crash recovery / WAL replay tests.

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

fn series_key(host: &str) -> Vec<u8> {
    storage_time_series::build_series_key(b"cpu", &[("host".to_string(), host.to_string())])
}

#[test]
fn wal_replays_on_reopen() {
    let dir = tempfile::tempdir().unwrap();
    {
        let engine = TimeSeriesEngine::open(dir.path(), opts()).unwrap();
        let a = series_key("a");
        let b = series_key("b");
        engine.put(a.clone(), 1, Value::F64(1.0)).unwrap();
        engine.put(b.clone(), 1, Value::F64(2.0)).unwrap();
        engine.put(a.clone(), 2, Value::F64(3.0)).unwrap();
        // Intentionally no sync/close; rely on WAL durability.
    }

    let engine = TimeSeriesEngine::open(dir.path(), opts()).unwrap();
    let a = series_key("a");
    let b = series_key("b");
    let a_latest = engine.get_latest(&a).unwrap().unwrap();
    assert_eq!(a_latest.value, Value::F64(3.0));
    let b_latest = engine.get_latest(&b).unwrap().unwrap();
    assert_eq!(b_latest.value, Value::F64(2.0));
}

#[test]
fn delete_series_replayed() {
    let dir = tempfile::tempdir().unwrap();
    {
        let engine = TimeSeriesEngine::open(dir.path(), opts()).unwrap();
        let a = series_key("a");
        engine.put(a.clone(), 1, Value::F64(1.0)).unwrap();
        engine.delete_series(&a).unwrap();
    }

    let engine = TimeSeriesEngine::open(dir.path(), opts()).unwrap();
    let a = series_key("a");
    assert!(engine.get_latest(&a).unwrap().is_none());
}

#[test]
fn sync_then_reopen_uses_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let key = series_key("a");
    {
        let engine = TimeSeriesEngine::open(dir.path(), opts()).unwrap();
        for i in 0..300u64 {
            engine.put(key.clone(), i, Value::F64(i as f64)).unwrap();
        }
        engine.sync().unwrap();
    }
    {
        let engine = TimeSeriesEngine::open(dir.path(), opts()).unwrap();
        let latest = engine.get_latest(&key).unwrap().unwrap();
        assert_eq!(latest.timestamp, 299);
        let range = engine.get_range(&key, 0, 300).unwrap();
        assert_eq!(range.len(), 300);
    }
}
