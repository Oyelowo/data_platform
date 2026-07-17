//! Configuration options for `storage-blob`.

use std::time::Duration;

/// Configuration for `BlobStoreImpl`.
#[derive(Debug, Clone)]
pub struct BlobStoreOptions {
    /// Maximum size of a single volume file in bytes.
    pub max_volume_size: u64,

    /// Threshold of dead bytes in a volume before GC considers rewriting it.
    /// Expressed as a ratio in `[0.0, 1.0]`.
    pub gc_dead_ratio_threshold: f64,

    /// Whether to run the GC worker in the background.
    pub background_gc: bool,

    /// Interval between automatic background GC passes.
    pub background_gc_interval: Duration,

    /// fsync policy for volume appends.
    pub sync_on_put: bool,
}

impl Default for BlobStoreOptions {
    fn default() -> Self {
        Self {
            max_volume_size: 256 * 1024 * 1024,
            gc_dead_ratio_threshold: 0.25,
            background_gc: true,
            background_gc_interval: Duration::from_secs(30),
            sync_on_put: true,
        }
    }
}

impl BlobStoreOptions {
    /// Validate option values.
    pub fn validate(&self) -> crate::Result<()> {
        if self.max_volume_size == 0 {
            return Err(crate::Error::InvalidOption(
                "max_volume_size must be > 0".into(),
            ));
        }
        if !(0.0..=1.0).contains(&self.gc_dead_ratio_threshold) {
            return Err(crate::Error::InvalidOption(
                "gc_dead_ratio_threshold must be in [0.0, 1.0]".into(),
            ));
        }
        if self.background_gc_interval.is_zero() {
            return Err(crate::Error::InvalidOption(
                "background_gc_interval must be > 0".into(),
            ));
        }
        Ok(())
    }
}
