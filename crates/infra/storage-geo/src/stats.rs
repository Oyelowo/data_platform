//! Engine statistics for the geospatial engine.

use std::collections::HashMap;

use storage_traits::EngineStats;

/// Snapshot of geospatial engine statistics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeoStats {
    /// Engine name.
    pub name: &'static str,
    /// Number of live features.
    pub num_features: u64,
    /// Approximate bytes stored on disk.
    pub disk_bytes: u64,
    /// Approximate bytes held in memory.
    pub memory_bytes: u64,
    /// Engine-specific counters.
    pub metrics: HashMap<String, u64>,
}

impl GeoStats {
    /// Convert into the generic `storage_traits::EngineStats`.
    pub fn into_engine_stats(self) -> EngineStats {
        EngineStats {
            name: self.name,
            disk_bytes: self.disk_bytes,
            memory_bytes: self.memory_bytes,
            num_keys: Some(self.num_features),
            metrics: self.metrics,
        }
    }
}
