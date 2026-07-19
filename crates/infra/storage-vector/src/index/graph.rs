//! Persistence helpers for approximate indexes.

use serde::{Deserialize, Serialize};

use crate::index::hnsw::Node;
use crate::options::{HnswOptions, IvfOptions};

/// Serializable HNSW graph state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HnswGraph {
    /// Options used to build the graph.
    pub options: HnswOptions,
    /// Entry point internal id.
    pub entry_point: Option<u64>,
    /// Maximum level in the graph.
    pub max_level: usize,
    /// Maps internal id to node layers.
    pub nodes: Vec<(u64, Node)>,
    /// Maps internal id to normalized vector.
    pub vectors: Vec<(u64, Vec<f32>)>,
}

/// Serializable IVF index state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IvfState {
    /// Options used to build the index.
    pub options: IvfOptions,
    /// Centroid vectors.
    pub centroids: Vec<Vec<f32>>,
    /// Inverted lists.
    pub lists: Vec<Vec<u64>>,
    /// Maps internal id to normalized vector.
    pub vectors: Vec<(u64, Vec<f32>)>,
    /// Maps internal id to centroid assignment.
    pub assignments: Vec<(u64, usize)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hnsw_graph_roundtrip() {
        let graph = HnswGraph {
            options: HnswOptions::default(),
            entry_point: Some(1),
            max_level: 2,
            nodes: vec![(1, Node { layers: vec![vec![2]] })],
            vectors: vec![(1, vec![1.0f32]), (2, vec![2.0f32])],
        };
        let encoded = bincode::serialize(&graph).unwrap();
        let decoded: HnswGraph = bincode::deserialize(&encoded).unwrap();
        assert_eq!(graph.entry_point, decoded.entry_point);
        assert_eq!(graph.max_level, decoded.max_level);
    }
}
