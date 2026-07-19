//! Crash recovery tests: reopen without explicit sync must replay WAL.

use storage_vector::{DistanceMetric, IndexType, VectorEngine, VectorOptions};

#[test]
fn wal_replays_on_reopen() {
    let dir = tempfile::tempdir().unwrap();
    {
        let engine = VectorEngine::open(
            dir.path(),
            VectorOptions::brute_force(4, DistanceMetric::Euclidean),
        )
        .unwrap();
        engine.put(b"a", &[1.0f32, 2.0, 3.0, 4.0]).unwrap();
        engine.put(b"b", &[4.0f32, 3.0, 2.0, 1.0]).unwrap();
        engine.delete(b"a").unwrap();
        // Intentionally no sync/close; rely on WAL durability.
    }

    let engine = VectorEngine::open(
        dir.path(),
        VectorOptions::brute_force(4, DistanceMetric::Euclidean),
    )
    .unwrap();
    assert!(engine.get(b"a").unwrap().is_none());
    let v = engine.get(b"b").unwrap().unwrap();
    assert_eq!(v, vec![4.0f32, 3.0, 2.0, 1.0]);
}

#[test]
fn index_rebuilt_after_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let opts = VectorOptions {
        dimension: 8,
        metric: DistanceMetric::Euclidean,
        index_type: IndexType::Hnsw,
        ..VectorOptions::default()
    };
    {
        let engine = VectorEngine::open(dir.path(), opts.clone()).unwrap();
        for i in 0..100u64 {
            let v: Vec<f32> = (0..8).map(|d| ((i + d) % 10) as f32).collect();
            engine.put(format!("k-{i}").as_bytes(), &v).unwrap();
        }
    }

    let engine = VectorEngine::open(dir.path(), opts).unwrap();
    let query: Vec<f32> = (0..8).map(|d| d as f32).collect();
    let results = engine.search(&query, 10).unwrap();
    assert_eq!(results.len(), 10);
}
