//! Inverted-file (IVF) approximate nearest-neighbor index.
//!
//! Vectors are clustered with k-means (k-means++ initialization + Lloyd
//! iterations). Search probes the `n_probe` nearest centroids and scans their
//! inverted lists.

use std::collections::HashMap;

use rand::Rng;

use crate::distance::{normalize, DistanceMetric};
use crate::format::VectorRecord;
use crate::index::{SearchResult, VectorIndex};
use crate::options::IvfOptions;

/// An IVF index.
pub struct IvfIndex {
    metric: DistanceMetric,
    options: IvfOptions,
    vectors: HashMap<u64, Vec<f32>>,
    centroids: Vec<Vec<f32>>,
    /// Inverted list: centroid index -> vector ids.
    lists: Vec<Vec<u64>>,
    /// Centroid assignment for each vector id.
    assignments: HashMap<u64, usize>,
}

impl IvfIndex {
    /// Create a new IVF index.
    pub fn new(metric: DistanceMetric, options: IvfOptions) -> Self {
        Self {
            metric,
            options,
            vectors: HashMap::new(),
            centroids: Vec::new(),
            lists: Vec::new(),
            assignments: HashMap::new(),
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
            DistanceMetric::Cosine => crate::distance::cosine_distance(a, b),
            DistanceMetric::DotProduct => crate::distance::neg_dot_product(a, b),
        }
    }

    fn assign(&self, vector: &[f32]) -> usize {
        self.centroids
            .iter()
            .enumerate()
            .map(|(i, c)| (i, crate::distance::euclidean(vector, c)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
            .unwrap_or(0)
    }

    fn kmeans_plus_plus(&self, vectors: &[Vec<f32>]) -> Vec<Vec<f32>> {
        let mut rng = rand::thread_rng();
        let k = self.options.n_clusters.min(vectors.len());
        let mut centroids: Vec<Vec<f32>> = Vec::with_capacity(k);
        let first = vectors[rng.gen_range(0..vectors.len())].clone();
        centroids.push(first);

        let mut distances: Vec<f32> = vectors
            .iter()
            .map(|v| crate::distance::euclidean(v, &centroids[0]).powi(2))
            .collect();

        for _ in 1..k {
            let total: f32 = distances.iter().sum();
            let target = if total > 0.0 {
                rng.r#gen::<f32>() * total
            } else {
                0.0
            };
            let mut cum = 0.0;
            let mut chosen = 0;
            for (i, &d) in distances.iter().enumerate() {
                cum += d;
                if cum >= target {
                    chosen = i;
                    break;
                }
            }
            centroids.push(vectors[chosen].clone());
            let last = centroids.last().expect("centroids is non-empty");
            for (i, v) in vectors.iter().enumerate() {
                let d = crate::distance::euclidean(v, last).powi(2);
                if d < distances[i] {
                    distances[i] = d;
                }
            }
        }
        centroids
    }

    fn train(&mut self, vectors: &[Vec<f32>]) {
        if vectors.len() <= self.options.n_clusters {
            self.centroids = vectors.to_vec();
            self.lists = vec![Vec::new(); self.centroids.len()];
            return;
        }

        let mut centroids = self.kmeans_plus_plus(vectors);
        let k = centroids.len();
        let dim = centroids[0].len();

        for _ in 0..self.options.max_iters {
            let mut sums = vec![vec![0.0f32; dim]; k];
            let mut counts = vec![0usize; k];
            for v in vectors {
                let idx = centroids
                    .iter()
                    .enumerate()
                    .map(|(i, c)| (i, crate::distance::euclidean(v, c)))
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                for (d, &val) in v.iter().enumerate() {
                    sums[idx][d] += val;
                }
                counts[idx] += 1;
            }
            for i in 0..k {
                if counts[i] > 0 {
                    for d in 0..dim {
                        centroids[i][d] = sums[i][d] / counts[i] as f32;
                    }
                }
            }
        }

        self.centroids = centroids;
        self.lists = vec![Vec::new(); k];
    }

    fn assign_all(&mut self) {
        for (&id, vector) in &self.vectors {
            let cluster = self.assign(vector);
            self.assignments.insert(id, cluster);
            self.lists[cluster].push(id);
        }
    }
}

impl VectorIndex for IvfIndex {
    fn search(&self, query: &[f32], k: usize, _ef: usize) -> Vec<SearchResult> {
        if self.centroids.is_empty() || self.vectors.is_empty() {
            return Vec::new();
        }
        let mut query = query.to_vec();
        self.prepare(&mut query);

        let mut centroid_dists: Vec<(usize, f32)> = self
            .centroids
            .iter()
            .enumerate()
            .map(|(i, c)| (i, crate::distance::euclidean(&query, c)))
            .collect();
        centroid_dists.sort_by(|a, b| a.1.total_cmp(&b.1));
        let probe = self.options.n_probe.min(centroid_dists.len());

        let mut results: Vec<SearchResult> = Vec::new();
        for &(cluster, _) in &centroid_dists[..probe] {
            for &id in &self.lists[cluster] {
                let vector = &self.vectors[&id];
                let dist = self.distance(&query, vector);
                if dist.is_finite() {
                    results.push(SearchResult { id, distance: dist });
                }
            }
        }
        results.sort_by(|a, b| a.distance.total_cmp(&b.distance));
        results.dedup_by(|a, b| a.id == b.id);
        results.truncate(k);
        results
    }

    fn insert(&mut self, record: &VectorRecord) {
        let mut vector = record.vector.clone();
        self.prepare(&mut vector);
        self.vectors.insert(record.id, vector);
        if self.centroids.is_empty() {
            if self.vectors.len() >= self.options.n_clusters {
                let all: Vec<Vec<f32>> = self.vectors.values().cloned().collect();
                self.train(&all);
                self.assign_all();
            }
            return;
        }
        let cluster = self.assign(&self.vectors[&record.id]);
        self.assignments.insert(record.id, cluster);
        self.lists[cluster].push(record.id);
    }

    fn delete(&mut self, id: u64) {
        if let Some(cluster) = self.assignments.remove(&id) {
            self.lists[cluster].retain(|&nid| nid != id);
        }
        self.vectors.remove(&id);
    }

    fn build(&mut self, records: &[VectorRecord]) {
        self.vectors.clear();
        self.assignments.clear();
        let mut prepared: Vec<Vec<f32>> = Vec::with_capacity(records.len());
        for rec in records {
            let mut v = rec.vector.clone();
            self.prepare(&mut v);
            self.vectors.insert(rec.id, v.clone());
            prepared.push(v);
        }
        self.train(&prepared);
        self.assign_all();
    }

    fn len(&self) -> usize {
        self.vectors.len()
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
    fn ivf_recall_vs_brute_force() {
        use crate::index::brute_force::BruteForceIndex;
        let mut ivf = IvfIndex::new(
            DistanceMetric::Euclidean,
            IvfOptions {
                n_clusters: 32,
                n_probe: 4,
                max_iters: 10,
            },
        );
        let mut brute = BruteForceIndex::new(DistanceMetric::Euclidean);
        let dim = 16usize;
        for i in 0..500u64 {
            let v: Vec<f32> = (0..dim)
                .map(|d| ((i as usize * 7 + d * 3) % 100) as f32 / 100.0)
                .collect();
            ivf.insert(&rec(i + 1, v.clone()));
            brute.insert(&rec(i + 1, v));
        }
        // Rebuild with training on the full set for better centroids.
        let records: Vec<VectorRecord> = (0..500u64)
            .map(|i| {
                let v: Vec<f32> =
                    (0..dim)
                        .map(|d| ((i as usize * 7 + d * 3) % 100) as f32 / 100.0)
                        .collect();
                rec(i + 1, v)
            })
            .collect();
        ivf.build(&records);

        let query: Vec<f32> = (0..dim).map(|d| d as f32 / 100.0).collect();
        let k = 10;
        let ivf_results: std::collections::HashSet<u64> =
            ivf.search(&query, k, 0).into_iter().map(|r| r.id).collect();
        let brute_results: std::collections::HashSet<u64> =
            brute.search(&query, k, 0).into_iter().map(|r| r.id).collect();
        let overlap = ivf_results.intersection(&brute_results).count();
        let recall = overlap as f32 / k as f32;
        assert!(recall >= 0.70, "IVF recall too low: {recall}");
    }
}
