//! Engine statistics conversion.

use storage_traits::EngineStats;

/// Time-series engine statistics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeSeriesStats {
    /// Engine name.
    pub name: &'static str,
    /// Approximate bytes stored on disk.
    pub disk_bytes: u64,
    /// Approximate bytes held in memory.
    pub memory_bytes: u64,
    /// Number of distinct series.
    pub num_series: u64,
    /// Number of memtable samples.
    pub memtable_samples: u64,
    /// Number of chunk files on disk.
    pub chunk_files: u64,
    /// Engine-specific counters.
    pub metrics: std::collections::HashMap<String, u64>,
}

impl TimeSeriesStats {
    /// Convert to the generic `EngineStats`.
    pub fn into_engine_stats(self) -> EngineStats {
        let mut metrics = self.metrics;
        metrics.insert("memtable_samples".into(), self.memtable_samples);
        metrics.insert("chunk_files".into(), self.chunk_files);
        metrics.insert("num_series".into(), self.num_series);
        EngineStats {
            name: self.name,
            disk_bytes: self.disk_bytes,
            memory_bytes: self.memory_bytes,
            num_keys: Some(self.num_series),
            metrics,
        }
    }
}
