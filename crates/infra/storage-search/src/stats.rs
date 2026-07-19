//! Statistics for `storage-search`.

use std::collections::HashMap;

/// Search engine statistics.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchStats {
    /// Engine name.
    pub name: &'static str,
    /// Number of indexed documents.
    pub num_docs: u64,
    /// Number of disk segments.
    pub num_segments: u64,
    /// Approximate disk bytes.
    pub disk_bytes: u64,
    /// Approximate memory bytes.
    pub memory_bytes: u64,
    /// Additional metrics.
    pub metrics: HashMap<String, u64>,
}

impl SearchStats {
    /// Convert to `storage_traits::EngineStats`.
    pub fn into_engine_stats(self) -> storage_traits::EngineStats {
        storage_traits::EngineStats {
            name: self.name,
            disk_bytes: self.disk_bytes,
            memory_bytes: self.memory_bytes,
            num_keys: Some(self.num_docs),
            metrics: self.metrics,
        }
    }
}
