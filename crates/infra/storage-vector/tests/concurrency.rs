//! Concurrent insert/search tests.

use std::sync::Arc;
use std::thread;

use storage_vector::{DistanceMetric, IndexType, VectorEngine, VectorOptions};

#[test]
fn concurrent_inserts_and_searches() {
    let dir = tempfile::tempdir().unwrap();
    let opts = VectorOptions {
        dimension: 16,
        metric: DistanceMetric::Euclidean,
        index_type: IndexType::Hnsw,
        ..VectorOptions::default()
    };
    let engine = Arc::new(VectorEngine::open(dir.path(), opts).unwrap());

    let mut handles = Vec::new();
    for t in 0..4 {
        let eng = Arc::clone(&engine);
        handles.push(thread::spawn(move || {
            for i in 0..100u64 {
                let base = t * 100 + i;
                let v: Vec<f32> = (0..16).map(|d| (base * 7 + d * 3) as f32 % 10.0).collect();
                eng.put(format!("t{t}-i{i}").as_bytes(), &v).unwrap();
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    assert_eq!(engine.stats().unwrap().num_vectors, 400);

    let query: Vec<f32> = (0..16).map(|d| d as f32).collect();
    let results = engine.search(&query, 10).unwrap();
    assert_eq!(results.len(), 10);
}
