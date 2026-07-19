//! HNSW recall and correctness tests.

use std::collections::HashSet;

use storage_vector::{
    DistanceMetric, HnswOptions, IndexType, VectorEngine, VectorOptions,
};

fn generate_vectors(count: usize, dim: usize, seed: u64) -> Vec<Vec<f32>> {
    use rand::Rng;
    let mut rng: rand::rngs::StdRng = rand::SeedableRng::seed_from_u64(seed);
    (0..count)
        .map(|i| {
            (0..dim)
                .map(|d| {
                    let x = ((i * 13 + d * 7) % 1000) as f32 / 1000.0;
                    x + rng.r#gen::<f32>() * 0.01
                })
                .collect()
        })
        .collect()
}

#[test]
fn hnsw_recall_euclidean() {
    let dir = tempfile::tempdir().unwrap();
    let opts = VectorOptions {
        dimension: 8,
        metric: DistanceMetric::Euclidean,
        index_type: IndexType::Hnsw,
        brute_force_threshold: 0,
        hnsw: HnswOptions {
            m: 16,
            ef_construction: 200,
            ef_search: 64,
            seed: 42,
            ..HnswOptions::default()
        },
        ..VectorOptions::default()
    };
    let engine = VectorEngine::open(dir.path(), opts).unwrap();

    let data = generate_vectors(300, 8, 42);
    for (i, v) in data.iter().enumerate() {
        engine.put(format!("vec-{i}").as_bytes(), v).unwrap();
    }

    let query = generate_vectors(1, 8, 9999).pop().unwrap();
    let k = 5;

    // Brute-force oracle.
    let mut oracle: Vec<(usize, f32)> = data
        .iter()
        .enumerate()
        .map(|(i, v)| (i, storage_vector::distance::euclidean(&query, v)))
        .collect();
    oracle.sort_by(|a, b| a.1.total_cmp(&b.1));
    let expected: HashSet<usize> = oracle.into_iter().take(k).map(|(i, _)| i).collect();

    let results = engine.search(&query, k).unwrap();
    eprintln!("results len={} first={:?}", results.len(), results.first());
    let got: HashSet<usize> = results
        .iter()
        .map(|r| {
            let key = engine.key_by_id(r.id).unwrap();
            let s = String::from_utf8(key).unwrap();
            s.strip_prefix("vec-")
                .unwrap()
                .parse::<usize>()
                .unwrap()
        })
        .collect();
    eprintln!("got={got:?}");
    eprintln!("expected={expected:?}");

    let overlap = got.intersection(&expected).count();
    let recall = overlap as f32 / k as f32;
    assert!(
        recall >= 0.80,
        "HNSW recall too low: {recall}"
    );
}
