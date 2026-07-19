//! Property-based tests: random puts + range queries validated against a model.

use std::collections::BTreeMap;

use proptest::prelude::*;
use storage_time_series::{
    CompressionKind, TimeSeriesEngine, TimeSeriesOptions, Value, WalSyncPolicy,
};

fn opts() -> TimeSeriesOptions {
    TimeSeriesOptions {
        compression: CompressionKind::Gorilla,
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

proptest! {
    #[test]
    fn random_puts_and_range_query(samples in proptest::collection::vec((0u64..100u64, 0.0f64..100.0f64), 1..200)) {
        let dir = tempfile::tempdir().unwrap();
        let engine = TimeSeriesEngine::open(dir.path(), opts()).unwrap();
        let key = series_key("db1");

        let mut model: BTreeMap<u64, f64> = BTreeMap::new();
        for (ts, value) in &samples {
            engine.put(key.clone(), *ts, Value::F64(*value)).unwrap();
            model.insert(*ts, *value);
        }
        engine.sync().unwrap();

        let range = engine.get_range(&key, 0, 100).unwrap();
        assert_eq!(range.len(), model.len());
        for (i, (ts, value)) in model.iter().enumerate() {
            assert_eq!(range[i].timestamp, *ts);
            if let Value::F64(v) = range[i].value {
                prop_assert!(v.is_finite());
                assert_eq!(v, *value);
            } else {
                panic!("expected f64 value");
            }
        }
    }
}
