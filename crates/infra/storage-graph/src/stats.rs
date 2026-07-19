//! Engine statistics for the graph storage engine.

use std::collections::HashMap;

use storage_traits::EngineStats;

/// Snapshot of graph engine statistics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphStats {
    /// Engine name.
    pub name: &'static str,
    /// Number of live nodes.
    pub num_nodes: u64,
    /// Number of live edges.
    pub num_edges: u64,
    /// Approximate bytes stored on disk.
    pub disk_bytes: u64,
    /// Approximate bytes held in memory.
    pub memory_bytes: u64,
    /// Engine-specific counters.
    pub metrics: HashMap<String, u64>,
}

impl GraphStats {
    /// Convert into the generic `storage_traits::EngineStats`.
    pub fn into_engine_stats(self) -> EngineStats {
        EngineStats {
            name: self.name,
            disk_bytes: self.disk_bytes,
            memory_bytes: self.memory_bytes,
            num_keys: Some(self.num_nodes + self.num_edges),
            metrics: self.metrics,
        }
    }
}
