//! Hierarchical Navigable Small World (HNSW) index.
//!
//! Implementation follows Malkov & Yashunin, "Efficient and robust approximate
//! nearest neighbor search using Hierarchical Navigable Small World graphs",
//! with standard heuristics for neighbor pruning.

use std::collections::{BinaryHeap, HashMap, HashSet};

use rand::Rng;
use rand::SeedableRng;

use crate::distance::{normalize, DistanceMetric};
use crate::format::VectorRecord;
use crate::index::{SearchResult, VectorIndex};
use crate::options::HnswOptions;

/// A node in the HNSW graph.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Node {
    /// Neighbors per layer, from layer 0 upward.
    pub layers: Vec<Vec<u64>>,
}

/// HNSW approximate nearest-neighbor index.
pub struct HnswIndex {
    metric: DistanceMetric,
    options: HnswOptions,
    nodes: HashMap<u64, Node>,
    vectors: HashMap<u64, Vec<f32>>,
    entry_point: Option<u64>,
    max_level: usize,
    rng: rand::rngs::StdRng,
}

/// Search candidate ordered for use as a **min-heap** by distance.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Candidate {
    distance: f32,
    id: u64,
}

impl Eq for Candidate {}

impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.distance.total_cmp(&other.distance)
    }
}

impl HnswIndex {
    /// Create a new HNSW index with the given metric and options.
    pub fn new(metric: DistanceMetric, options: HnswOptions) -> Self {
        let rng = if options.seed == 0 {
            rand::rngs::StdRng::from_entropy()
        } else {
            rand::rngs::StdRng::seed_from_u64(options.seed)
        };
        Self {
            metric,
            options,
            nodes: HashMap::new(),
            vectors: HashMap::new(),
            entry_point: None,
            max_level: 0,
            rng,
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

    fn m_max_for_level(&self, level: usize) -> usize {
        if level == 0 {
            self.options.m * 2
        } else {
            self.options.m
        }
    }

    fn random_level(&mut self) -> usize {
        let uniform: f64 = self.rng.r#gen();
        let level = (-uniform.ln() * self.options.level_multiplier).floor() as usize;
        level.min(self.options.max_level)
    }

    /// Exact brute-force search over all indexed vectors.
    fn brute_force_search(&self, query: &[f32], k: usize) -> Vec<SearchResult> {
        let mut heap = std::collections::BinaryHeap::new();
        for (&id, vector) in &self.vectors {
            let dist = self.distance(query, vector);
            heap.push(SearchResult { id, distance: dist });
            if heap.len() > k {
                heap.pop();
            }
        }
        let mut results: Vec<SearchResult> = heap.into_iter().collect();
        results.sort_by(|a, b| a.distance.total_cmp(&b.distance));
        results
    }

    /// Search a single layer starting from `entry`, returning up to `ef` candidates.
    fn search_layer(
        &self,
        query: &[f32],
        entry: u64,
        ef: usize,
        level: usize,
    ) -> Vec<SearchResult> {
        let mut visited = HashSet::new();
        // Candidates is a min-heap: expand the closest unexplored node next.
        let mut candidates: BinaryHeap<Candidate> = BinaryHeap::new();
        // Top is a max-heap by distance so we can evict the worst candidate
        // when it grows beyond `ef`.
        let mut top: BinaryHeap<Candidate> = BinaryHeap::new();

        let entry_dist = self.distance(query, &self.vectors[&entry]);
        visited.insert(entry);
        candidates.push(Candidate {
            distance: entry_dist,
            id: entry,
        });
        top.push(Candidate {
            distance: entry_dist,
            id: entry,
        });

        while let Some(curr) = candidates.pop() {
            if let Some(worst) = top.peek()
                && top.len() >= ef && curr.distance > worst.distance
            {
                break;
            }
            let node = match self.nodes.get(&curr.id) {
                Some(n) => n,
                None => continue,
            };
            let neighbors = node.layers.get(level).map(|l| l.as_slice()).unwrap_or(&[]);
            for &neighbor in neighbors {
                if visited.insert(neighbor) {
                    let dist = self.distance(query, &self.vectors[&neighbor]);
                    let should_add = top
                        .peek()
                        .map(|worst| dist < worst.distance)
                        .unwrap_or(true);
                    if top.len() < ef || should_add {
                        candidates.push(Candidate { distance: dist, id: neighbor });
                        top.push(Candidate { distance: dist, id: neighbor });
                        if top.len() > ef {
                            top.pop();
                        }
                    }
                }
            }
        }

        let mut results: Vec<SearchResult> = top
            .into_iter()
            .map(|c| SearchResult {
                id: c.id,
                distance: c.distance,
            })
            .collect();
        results.sort_by(|a, b| a.distance.total_cmp(&b.distance));
        results
    }

    /// Select the `m` nearest neighbors from a list of candidates.
    fn select_neighbors(&self, _query: &[f32], candidates: &[SearchResult], m: usize) -> Vec<u64> {
        let mut sorted = candidates.to_vec();
        sorted.sort_by(|a, b| a.distance.total_cmp(&b.distance));
        sorted.into_iter().take(m).map(|r| r.id).collect()
    }

    /// Insert a node into the graph.
    pub fn insert_node(&mut self, id: u64, vector: &[f32], level: usize) {
        let mut prepared = vector.to_vec();
        self.prepare(&mut prepared);
        self.vectors.insert(id, prepared.clone());

        let node = Node {
            layers: vec![Vec::new(); level + 1],
        };
        self.nodes.insert(id, node);

        let Some(entry) = self.entry_point else {
            self.entry_point = Some(id);
            self.max_level = level;
            return;
        };

        // 1. Find the closest entry point for layers above the new node's level.
        let mut current_ep = entry;
        for l in (level + 1..=self.max_level).rev() {
            let res = self.search_layer(&prepared, current_ep, 1, l);
            if let Some(best) = res.first() {
                current_ep = best.id;
            }
        }

        // 2. Insert at the new node's level and below.
        for l in (0..=level.min(self.max_level)).rev() {
            let ef = self.options.ef_construction.max(self.options.m);
            let mut candidates = self.search_layer(&prepared, current_ep, ef, l);
            // Filter out the query itself if it already existed (re-insert).
            candidates.retain(|c| c.id != id);
            let neighbors = self.select_neighbors(&prepared, &candidates, self.options.m);
            {
                let node = self
                    .nodes
                    .get_mut(&id)
                    .expect("node for inserted id must exist");
                node.layers[l] = neighbors.clone();
            }
            for &neighbor in &neighbors {
                let m_max = self.m_max_for_level(l);
                let n_vec = self.vectors[&neighbor].clone();
                let mut layer = self.nodes[&neighbor].layers[l].clone();
                layer.push(id);
                if layer.len() > m_max {
                    let mut scored: Vec<(f32, u64)> = layer
                        .iter()
                        .map(|&nid| (self.distance(&n_vec, &self.vectors[&nid]), nid))
                        .collect();
                    scored.sort_by(|a, b| a.0.total_cmp(&b.0));
                    layer = scored.into_iter().take(m_max).map(|(_, nid)| nid).collect();
                }
                let neighbor_node = self
                    .nodes
                    .get_mut(&neighbor)
                    .expect("neighbor node must exist");
                neighbor_node.layers[l] = layer;
            }
            if l > 0 {
                // Next level's entry point is the closest candidate at this level.
                let res = self.search_layer(&prepared, current_ep, 1, l);
                if let Some(best) = res.first() {
                    current_ep = best.id;
                }
            }
        }

        if level > self.max_level {
            self.max_level = level;
            self.entry_point = Some(id);
        }
    }
}

impl VectorIndex for HnswIndex {
    fn search(&self, query: &[f32], k: usize, ef: usize) -> Vec<SearchResult> {
        let mut query = query.to_vec();
        self.prepare(&mut query);

        let graph_results = if let Some(entry) = self.entry_point {
            let ef = ef.max(k);
            let mut current_ep = entry;
            for l in (1..=self.max_level).rev() {
                let res = self.search_layer(&query, current_ep, 1, l);
                if let Some(best) = res.first() {
                    current_ep = best.id;
                }
            }

            let mut candidates = self.search_layer(&query, current_ep, ef, 0);
            candidates.retain(|c| c.distance.is_finite());
            candidates
        } else {
            Vec::new()
        };

        // Fallback to a full scan if the graph returned too few candidates.
        // This keeps small or disconnected graphs correct while preserving
        // HNSW performance on large, well-formed graphs.
        if graph_results.len() < k {
            return self.brute_force_search(&query, k);
        }

        let mut results = graph_results;
        results.truncate(k);
        results
    }

    fn insert(&mut self, record: &VectorRecord) {
        let level = self.random_level();
        self.insert_node(record.id, &record.vector, level);
    }

    fn delete(&mut self, id: u64) {
        self.nodes.remove(&id);
        self.vectors.remove(&id);
        for node in self.nodes.values_mut() {
            for layer in &mut node.layers {
                layer.retain(|&nid| nid != id);
            }
        }
        if self.entry_point == Some(id) {
            self.entry_point = self.vectors.keys().copied().next();
            self.max_level = self
                .entry_point
                .and_then(|ep| self.nodes.get(&ep).map(|n| n.layers.len().saturating_sub(1)))
                .unwrap_or(0);
        }
    }

    fn build(&mut self, records: &[VectorRecord]) {
        self.nodes.clear();
        self.vectors.clear();
        self.entry_point = None;
        self.max_level = 0;
        for rec in records {
            self.insert(rec);
        }
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

    fn seeded_opts() -> HnswOptions {
        HnswOptions {
            seed: 42,
            ef_construction: 300,
            ef_search: 128,
            m: 24,
            ..HnswOptions::default()
        }
    }

    #[test]
    fn hnsw_search_finds_neighbors() {
        let mut idx = HnswIndex::new(DistanceMetric::Euclidean, seeded_opts());
        for i in 0..50u64 {
            idx.insert(&rec(i + 1, vec![i as f32, i as f32 * 2.0]));
        }
        let results = idx.search(&[0.0f32, 0.0], 5, 16);
        assert_eq!(results.len(), 5);
        assert_eq!(results[0].id, 1);
    }

    #[test]
    fn hnsw_recall_vs_brute_force() {
        use crate::index::brute_force::BruteForceIndex;
        let mut hnsw = HnswIndex::new(DistanceMetric::Euclidean, seeded_opts());
        let mut brute = BruteForceIndex::new(DistanceMetric::Euclidean);
        let dim = 16usize;
        for i in 0..500u64 {
            let v: Vec<f32> = (0..dim)
                .map(|d| ((i as usize * 7 + d * 3) % 100) as f32 / 100.0)
                .collect();
            hnsw.insert(&rec(i + 1, v.clone()));
            brute.insert(&rec(i + 1, v));
        }

        let query: Vec<f32> = (0..dim).map(|d| d as f32 / 100.0).collect();
        let k = 10;
        let hnsw_results: HashSet<u64> = hnsw.search(&query, k, 64).into_iter().map(|r| r.id).collect();
        let brute_results: HashSet<u64> = brute.search(&query, k, 0).into_iter().map(|r| r.id).collect();
        let overlap = hnsw_results.intersection(&brute_results).count();
        let recall = overlap as f32 / k as f32;
        assert!(
            recall >= 0.85,
            "HNSW recall too low: {recall}"
        );
    }
}
