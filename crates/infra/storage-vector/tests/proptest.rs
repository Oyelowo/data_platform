//! Property-based tests for `storage-vector`.

use proptest::prelude::*;
use storage_vector::{DistanceMetric, IndexType, VectorEngine, VectorOptions};

fn small_vector() -> impl Strategy<Value = Vec<f32>> {
    prop::collection::vec(-10.0f32..=10.0f32, 4)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn put_get_roundtrip(vectors in prop::collection::vec((any::<[u8; 8]>(), small_vector()), 1..20)) {
        let dir = tempfile::tempdir().unwrap();
        let engine = VectorEngine::open(
            dir.path(),
            VectorOptions::brute_force(4, DistanceMetric::Euclidean),
        ).unwrap();

        for (key, vector) in &vectors {
            engine.put(key, vector).unwrap();
        }

        for (key, vector) in &vectors {
            let got = engine.get(key).unwrap().unwrap();
            prop_assert_eq!(&got, vector);
        }
    }

    #[test]
    fn delete_then_get_none(vectors in prop::collection::vec((any::<[u8; 4]>(), small_vector()), 1..10), idx in 0usize..10) {
        let dir = tempfile::tempdir().unwrap();
        let engine = VectorEngine::open(
            dir.path(),
            VectorOptions::brute_force(4, DistanceMetric::Euclidean),
        ).unwrap();

        for (key, vector) in &vectors {
            engine.put(key, vector).unwrap();
        }

        if let Some((key, _)) = vectors.get(idx) {
            engine.delete(key).unwrap();
            prop_assert!(engine.get(key).unwrap().is_none());
        }
    }
}

#[test]
fn brute_force_oracle_matches_hnsw_on_small_dataset() {
    let dir = tempfile::tempdir().unwrap();
    let opts = VectorOptions {
        dimension: 4,
        metric: DistanceMetric::Euclidean,
        index_type: IndexType::Hnsw,
        ..VectorOptions::default()
    };
    let engine = VectorEngine::open(dir.path(), opts).unwrap();
    for i in 0..30u64 {
        let v = vec![i as f32, (i * 2) as f32, (i * 3) as f32, (i * 4) as f32];
        engine.put(format!("k{i}").as_bytes(), &v).unwrap();
    }
    let query = vec![5.0f32, 10.0, 15.0, 20.0];
    let results = engine.search(&query, 5).unwrap();
    assert!(!results.is_empty());
    // On a tiny deterministic dataset the exact match (id=5) must be returned.
    let ids: std::collections::HashSet<u64> = results.iter().map(|r| r.id).collect();
    assert!(ids.contains(&5), "exact neighbor not found; got {ids:?}");
}
