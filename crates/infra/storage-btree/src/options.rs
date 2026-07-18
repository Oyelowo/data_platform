//! Configuration options for the in-place B+ tree engine.

use std::sync::Arc;
use std::time::Duration;

use crate::io::{FaultSchedule, StorageBackend};

/// Options used when opening a [`BtreeEngine`](crate::engine::BtreeEngine).
#[derive(Clone)]
pub struct BtreeOptions {
    /// Size of each on-disk page in bytes.
    ///
    /// Must be a power of two and at least 512. The default is 4096.
    pub page_size: usize,

    /// Maximum size of an inline value in bytes.
    ///
    /// Values larger than this are stored in the append-only value log. The
    /// default is one quarter of the page size.
    pub max_inline_value_size: usize,

    /// Minimum fill ratio for a node after deletion, expressed as a percentage.
    ///
    /// The default is 50. The engine translates this into a minimum live-cell
    /// count using an assumed average record size of 64 bytes; engines with
    /// extremely large or small keys may want to set [`min_cells`](Self::min_cells)
    /// directly.
    pub min_fill_percent: usize,

    /// Explicit minimum number of live cells a non-root page must retain.
    ///
    /// If set, this overrides the value derived from `min_fill_percent`.
    pub min_cells: Option<usize>,

    /// Maximum size of the in-memory page cache in bytes.
    ///
    /// A value of zero is treated as the default (64 MiB). The cache capacity is
    /// rounded down to a whole number of pages, with a minimum of 16 frames.
    pub cache_size: usize,

    /// Maximum size of a single value in bytes.
    ///
    /// Values larger than this are rejected. The default is 16 MiB.
    pub max_value_size: usize,

    /// Maximum number of operations allowed in a single multi-record transaction.
    ///
    /// The default is 10,000.
    pub max_batch_ops: usize,

    /// Interval between automatic fuzzy checkpoints.
    ///
    /// `None` disables the background checkpoint thread; `sync()` and `close()`
    /// still run checkpoints synchronously. The default is `None`.
    pub background_checkpoint_interval: Option<Duration>,

    /// Interval between background page-cleaner wakes.
    ///
    /// The cleaner flushes dirty, unpinned frames between checkpoints so that
    /// foreground eviction rarely has to write synchronously. `None` disables
    /// the cleaner. The default is `None`.
    pub background_page_cleaner_interval: Option<Duration>,

    /// Optional fault-injection configuration for the WAL committer.
    ///
    /// This is intended for deterministic durability testing. The default is
    /// `None`.
    #[cfg_attr(not(test), allow(dead_code))]
    pub wal_fault_config: Option<storage_wal::FaultConfig>,

    /// Optional storage backend to use instead of the production `RealBackend`.
    ///
    /// This is primarily useful for tests that inject faults through a
    /// [`FaultyBackend`](crate::io::FaultyBackend). The default is `None`, which
    /// uses `RealBackend`.
    #[cfg_attr(not(test), allow(dead_code))]
    pub backend: Option<Arc<dyn StorageBackend>>,

    /// Optional fault schedule. When set, the engine wraps the configured (or
    /// default) backend in a `FaultyBackend`.
    ///
    /// The default is `None`.
    pub fault_schedule: Option<FaultSchedule>,

    /// Run the online integrity check after each automatic checkpoint.
    ///
    /// This is useful in tests and during early production deployments. It has
    /// a performance cost, so it defaults to `false`.
    pub checkpoint_integrity_check: bool,

    /// Compact the value log after each automatic checkpoint.
    ///
    /// The default is `false` because value-log compaction is stop-the-world and
    /// may be scheduled explicitly instead.
    pub checkpoint_compact_value_log: bool,
}

impl BtreeOptions {
    /// Average record size used when translating `min_fill_percent` into a cell
    /// count. This is intentionally conservative for mixed key/value sizes.
    const ASSUMED_RECORD_SIZE: usize = 64;

    /// Validate options and fill in derived defaults.
    pub(crate) fn validate(&self) -> Result<Self, crate::Error> {
        if self.page_size < 512 {
            return Err(crate::Error::InvalidArgument(
                "page_size must be at least 512".into(),
            ));
        }
        if self.page_size.count_ones() != 1 {
            return Err(crate::Error::InvalidArgument(
                "page_size must be a power of two".into(),
            ));
        }
        if self.min_fill_percent == 0 || self.min_fill_percent > 100 {
            return Err(crate::Error::InvalidArgument(
                "min_fill_percent must be in (0, 100]".into(),
            ));
        }
        if self.max_value_size == 0 {
            return Err(crate::Error::InvalidArgument(
                "max_value_size must be non-zero".into(),
            ));
        }
        if self.max_batch_ops == 0 {
            return Err(crate::Error::InvalidArgument(
                "max_batch_ops must be non-zero".into(),
            ));
        }
        let max_inline = self.max_inline_value_size.min(self.page_size / 4);
        if max_inline == 0 {
            return Err(crate::Error::InvalidArgument(
                "max_inline_value_size is too small for the configured page_size".into(),
            ));
        }
        Ok(Self {
            page_size: self.page_size,
            max_inline_value_size: max_inline,
            min_fill_percent: self.min_fill_percent,
            min_cells: self.min_cells,
            cache_size: if self.cache_size == 0 {
                64 * 1024 * 1024
            } else {
                self.cache_size
            },
            max_value_size: self.max_value_size,
            max_batch_ops: self.max_batch_ops,
            background_checkpoint_interval: self.background_checkpoint_interval,
            background_page_cleaner_interval: self.background_page_cleaner_interval,
            wal_fault_config: self.wal_fault_config.clone(),
            backend: self.backend.clone(),
            fault_schedule: self.fault_schedule.clone(),
            checkpoint_integrity_check: self.checkpoint_integrity_check,
            checkpoint_compact_value_log: self.checkpoint_compact_value_log,
        })
    }

    /// Number of frames to allocate in the buffer pool for these options.
    pub(crate) fn cache_frames(&self) -> usize {
        (self.cache_size / self.page_size).max(16)
    }

    /// Inline value threshold in bytes.
    pub(crate) fn inline_threshold(&self) -> usize {
        self.max_inline_value_size
    }

    /// Minimum live cells derived from `min_fill_percent` or overridden by
    /// `min_cells`.
    pub(crate) fn min_cells(&self) -> usize {
        if let Some(n) = self.min_cells {
            return n.max(1);
        }
        let usable = self.page_size.saturating_sub(crate::page::HEADER_SIZE);
        let target_bytes = usable * self.min_fill_percent / 100;
        let cells = target_bytes / Self::ASSUMED_RECORD_SIZE;
        cells.max(1)
    }

    /// Options for the physiological WAL.
    pub(crate) fn wal_options(&self) -> storage_wal::WalOptions {
        storage_wal::WalOptions {
            // Segment must be large enough for the largest value the engine is
            // expected to store (the test suite writes 1 MiB values).
            segment_size: (self.page_size as u64 * 256).max(2 * 1024 * 1024),
            ..Default::default()
        }
    }
}

impl Default for BtreeOptions {
    fn default() -> Self {
        Self {
            page_size: 4096,
            max_inline_value_size: 1024,
            min_fill_percent: 50,
            min_cells: None,
            cache_size: 64 * 1024 * 1024,
            max_value_size: 16 * 1024 * 1024,
            max_batch_ops: 10_000,
            background_checkpoint_interval: None,
            background_page_cleaner_interval: None,
            wal_fault_config: None,
            backend: None,
            fault_schedule: None,
            checkpoint_integrity_check: false,
            checkpoint_compact_value_log: false,
        }
    }
}

impl std::fmt::Debug for BtreeOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BtreeOptions")
            .field("page_size", &self.page_size)
            .field("max_inline_value_size", &self.max_inline_value_size)
            .field("min_fill_percent", &self.min_fill_percent)
            .field("min_cells", &self.min_cells)
            .field("cache_size", &self.cache_size)
            .field("max_value_size", &self.max_value_size)
            .field("max_batch_ops", &self.max_batch_ops)
            .field(
                "background_checkpoint_interval",
                &self.background_checkpoint_interval,
            )
            .field(
                "background_page_cleaner_interval",
                &self.background_page_cleaner_interval,
            )
            .field("wal_fault_config", &self.wal_fault_config)
            .field("backend", &self.backend.as_ref().map(|_| ".."))
            .field("fault_schedule", &self.fault_schedule)
            .field(
                "checkpoint_integrity_check",
                &self.checkpoint_integrity_check,
            )
            .field(
                "checkpoint_compact_value_log",
                &self.checkpoint_compact_value_log,
            )
            .finish()
    }
}

impl PartialEq for BtreeOptions {
    fn eq(&self, other: &Self) -> bool {
        self.page_size == other.page_size
            && self.max_inline_value_size == other.max_inline_value_size
            && self.min_fill_percent == other.min_fill_percent
            && self.min_cells == other.min_cells
            && self.cache_size == other.cache_size
            && self.max_value_size == other.max_value_size
            && self.max_batch_ops == other.max_batch_ops
            && self.background_checkpoint_interval == other.background_checkpoint_interval
            && self.background_page_cleaner_interval == other.background_page_cleaner_interval
            && self.wal_fault_config == other.wal_fault_config
            && self.fault_schedule == other.fault_schedule
            && self.checkpoint_integrity_check == other.checkpoint_integrity_check
            && self.checkpoint_compact_value_log == other.checkpoint_compact_value_log
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        let opts = BtreeOptions::default();
        let validated = opts.validate().unwrap();
        assert_eq!(validated.page_size, 4096);
        assert_eq!(validated.inline_threshold(), 1024);
        assert_eq!(validated.cache_frames(), 16384);
        assert!(validated.min_cells() >= 1);
    }

    #[test]
    fn invalid_page_size_rejected() {
        let opts = BtreeOptions {
            page_size: 100,
            ..Default::default()
        };
        assert!(opts.validate().is_err());
        let opts = BtreeOptions {
            page_size: 3 * 1024,
            ..Default::default()
        };
        assert!(opts.validate().is_err());
    }

    #[test]
    fn zero_cache_size_uses_default() {
        let opts = BtreeOptions {
            cache_size: 0,
            ..Default::default()
        };
        let validated = opts.validate().unwrap();
        assert_eq!(validated.cache_size, 64 * 1024 * 1024);
    }

    #[test]
    fn min_cells_override_wins() {
        let opts = BtreeOptions {
            min_cells: Some(7),
            ..Default::default()
        };
        assert_eq!(opts.validate().unwrap().min_cells(), 7);
    }
}
