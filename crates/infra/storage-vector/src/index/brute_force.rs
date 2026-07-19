//! Exact brute-force nearest-neighbor index.

use std::collections::HashMap;

use crate::distance::{cosine_distance, normalize, DistanceMetric};
use crate::format::VectorRecord;
use crate::index::{SearchResult, VectorIndex};

/// Exact brute-force index. Serves as the correctness oracle for approximate
/// indexes and is efficient for small datasets.
pub struct BruteForceIndex {
    metric: DistanceMetric,
    records: HashMap<u64, Vec<f32>>,
}

impl BruteForceIndex {
    /// Create a new brute-force index.
    pub fn new(metric: DistanceMetric) -> Self {
        Self {
            metric,
            records: HashMap::new(),
        }
    }

    fn prepare(&self, vector: &mut [f32]) {
        if self.metric == DistanceMetric::Cosine {
            normalize(vector);
        }
    }

    fn distance(&self, a: &[f32], b: &[f32]) -> f32 {
        match self.metric {
            DistanceMetric::Euclidean => crate::distance::euclidean(a, b),
            DistanceMetric::Cosine => cosine_distance(a, b),
            DistanceMetric::DotProduct => crate::distance::neg_dot_product(a, b),
        }
    }
}

impl VectorIndex for BruteForceIndex {
    fn search(&self, query: &[f32], k: usize, _ef: usize) -> Vec<SearchResult> {
        let mut query = query.to_vec();
        self.prepare(&mut query);
        let mut heap: std::collections::BinaryHeap<SearchResult> =
            std::collections::BinaryHeap::new();
        for (&id, vector) in &self.records {
            let dist = self.distance(&query, vector);
            heap.push(SearchResult { id, distance: dist });
            if heap.len() > k {
                heap.pop();
            }
        }
        let mut results: Vec<SearchResult> = heap.into_iter().collect();
        results.sort_by(|a, b| a.distance.total_cmp(&b.distance));
        results
    }

    fn insert(&mut self, record: &VectorRecord) {
        let mut vector = record.vector.clone();
        self.prepare(&mut vector);
        self.records.insert(record.id, vector);
    }

    fn delete(&mut self, id: u64) {
        self.records.remove(&id);
    }

    fn build(&mut self, records: &[VectorRecord]) {
        self.records.clear();
        for rec in records {
            self.insert(rec);
        }
    }

    fn len(&self) -> usize {
        self.records.len()
    }

    fn metric(&self) -> DistanceMetric {
        self.metric
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(id: u64, v: Vec<f32>) -> VectorRecord {
        VectorRecord {
            id,
            key: id.to_le_bytes().to_vec(),
            vector: v,
        }
    }

    #[test]
    fn brute_force_top_k() {
        let mut idx = BruteForceIndex::new(DistanceMetric::Euclidean);
        idx.insert(&rec(1, vec![0.0f32, 0.0]));
        idx.insert(&rec(2, vec![3.0f32, 4.0]));
        idx.insert(&rec(3, vec![1.0f32, 1.0]));
        let results = idx.search(&[0.0f32, 0.0], 2, 0);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, 1);
        assert_eq!(results[1].id, 3);
    }

    #[test]
    fn brute_force_cosine_normalized() {
        let mut idx = BruteForceIndex::new(DistanceMetric::Cosine);
        idx.insert(&rec(1, vec![1.0f32, 0.0]));
        idx.insert(&rec(2, vec![0.0f32, 1.0]));
        let results = idx.search(&[1.0f32, 0.0], 1, 0);
        assert_eq!(results[0].id, 1);
        assert!(results[0].distance.abs() < 1e-6);
    }
}
