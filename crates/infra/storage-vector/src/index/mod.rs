//! Approximate nearest-neighbor index implementations.

pub mod brute_force;
pub mod graph;
pub mod hnsw;
pub mod ivf;

pub use brute_force::BruteForceIndex;
pub use hnsw::HnswIndex;
pub use ivf::IvfIndex;

use crate::distance::DistanceMetric;
use crate::format::VectorRecord;

/// A single nearest-neighbor result.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SearchResult {
    /// Internal vector id.
    pub id: u64,
    /// Distance from the query according to the index metric.
    pub distance: f32,
}

impl PartialOrd for SearchResult {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Eq for SearchResult {}

impl Ord for SearchResult {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.distance.total_cmp(&other.distance)
    }
}

/// Common interface for vector indexes.
pub trait VectorIndex: Send + Sync {
    /// Search the index for the `k` nearest neighbors of `query`.
    ///
    /// `ef` is an expansion factor whose meaning depends on the index:
    /// * brute force: ignored;
    /// * HNSW: size of the candidate list;
    /// * IVF: unused (use `n_probe` from options instead).
    fn search(&self, query: &[f32], k: usize, ef: usize) -> Vec<SearchResult>;

    /// Insert a vector into the index.
    fn insert(&mut self, record: &VectorRecord);

    /// Delete a vector from the index.
    fn delete(&mut self, id: u64);

    /// Rebuild the index from a slice of records.
    fn build(&mut self, records: &[VectorRecord]);

    /// Return the number of vectors in the index.
    fn len(&self) -> usize;

    /// Return whether the index is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Return the distance metric used by the index.
    fn metric(&self) -> DistanceMetric;
}
