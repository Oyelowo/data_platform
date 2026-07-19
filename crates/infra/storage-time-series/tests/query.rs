//! Range and aggregation query tests.

use storage_time_series::{
    CompressionKind, Query, TimeSeriesEngine, TimeSeriesOptions, Value, WalSyncPolicy,
};
use storage_time_series::query::Aggregation;

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
fn query_range_filters_by_tags() {
    let dir = tempfile::tempdir().unwrap();
    let engine = TimeSeriesEngine::open(dir.path(), opts()).unwrap();
    let a = series_key(b"cpu", "a");
    let b = series_key(b"cpu", "b");
    for i in 0..10u64 {
        engine.put(a.clone(), i, Value::F64(i as f64)).unwrap();
        engine.put(b.clone(), i, Value::F64((i * 10) as f64)).unwrap();
    }
    engine.sync().unwrap();

    let result = engine
        .query(Query::new(b"cpu").eq("host", "a").range(3, 8))
        .unwrap();
    assert_eq!(result.series.len(), 1);
    let samples = result.series.get(&a).unwrap();
    assert_eq!(samples.len(), 5); // 3,4,5,6,7
    assert_eq!(samples[0].timestamp, 3);
}

#[test]
fn query_aggregations() {
    let dir = tempfile::tempdir().unwrap();
    let engine = TimeSeriesEngine::open(dir.path(), opts()).unwrap();
    let key = series_key(b"cpu", "a");
    for i in 0..10u64 {
        engine.put(key.clone(), i, Value::F64(i as f64)).unwrap();
    }
    engine.sync().unwrap();

    for (agg, expected) in [
        (Aggregation::Sum, 45.0),
        (Aggregation::Count, 10.0),
        (Aggregation::Avg, 4.5),
        (Aggregation::Min, 0.0),
        (Aggregation::Max, 9.0),
    ] {
        let result = engine
            .query(Query::new(b"cpu").eq("host", "a").range(0, 10).aggregate(agg))
            .unwrap();
        let agg_result = result.aggregates.unwrap().get(&key).copied().unwrap();
        assert_eq!(agg_result, storage_time_series::query::AggregateResult::Scalar(expected));
    }
}

#[test]
fn query_neq_filter() {
    let dir = tempfile::tempdir().unwrap();
    let engine = TimeSeriesEngine::open(dir.path(), opts()).unwrap();
    let a = series_key(b"cpu", "a");
    let b = series_key(b"cpu", "b");
    engine.put(a.clone(), 1, Value::F64(1.0)).unwrap();
    engine.put(b.clone(), 1, Value::F64(2.0)).unwrap();
    engine.sync().unwrap();

    let result = engine
        .query(Query::new(b"cpu").neq("host", "a"))
        .unwrap();
    assert_eq!(result.series.len(), 1);
    assert!(result.series.contains_key(&b));
}
