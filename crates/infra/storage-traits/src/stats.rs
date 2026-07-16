//! Engine statistics and metrics types.

/// Snapshot of engine-level statistics.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EngineStats {
    /// Engine name.
    pub name: &'static str,
    /// Approximate bytes stored on disk.
    pub disk_bytes: u64,
    /// Approximate bytes held in memory.
    pub memory_bytes: u64,
    /// Number of keys, if known cheaply.
    pub num_keys: Option<u64>,
}
