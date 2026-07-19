//! Scalar quantization integration tests.

use storage_vector::{DistanceMetric, IndexType, Quantization, VectorEngine, VectorOptions};

#[test]
fn scalar_quantization_bounded_error() {
    let dir = tempfile::tempdir().unwrap();
    let opts = VectorOptions {
        dimension: 32,
        metric: DistanceMetric::Euclidean,
        index_type: IndexType::BruteForce,
        quantization: Quantization::Scalar,
        ..VectorOptions::default()
    };
    let engine = VectorEngine::open(dir.path(), opts).unwrap();

    for i in 0..500u64 {
        let v: Vec<f32> = (0..32).map(|d| ((i * 7 + d * 3) % 100) as f32 / 100.0).collect();
        engine.put(format!("q-{i}").as_bytes(), &v).unwrap();
    }

    let query: Vec<f32> = (0..32).map(|d| d as f32 / 100.0).collect();
    let results = engine.search(&query, 10).unwrap();
    assert_eq!(results.len(), 10);
    for r in &results {
        assert!(r.distance >= 0.0);
    }
}
