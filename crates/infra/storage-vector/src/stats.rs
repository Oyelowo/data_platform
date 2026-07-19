//! Engine statistics for the vector store.

use std::collections::HashMap;

/// Statistics snapshot for a vector engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorStats {
    /// Engine name.
    pub name: &'static str,
    /// Number of stored vectors.
    pub num_vectors: u64,
    /// Configured vector dimension.
    pub dimension: u64,
    /// Approximate bytes stored on disk.
    pub disk_bytes: u64,
    /// Approximate bytes held in memory.
    pub memory_bytes: u64,
    /// Engine-specific counters.
    pub metrics: HashMap<String, u64>,
}

impl Default for VectorStats {
    fn default() -> Self {
        Self {
            name: "storage-vector",
            num_vectors: 0,
            dimension: 0,
            disk_bytes: 0,
            memory_bytes: 0,
            metrics: HashMap::new(),
        }
    }
}
