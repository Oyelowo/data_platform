//! Configuration options for the LSM engine.

use std::sync::Arc;

use crate::logger::{Logger, noop_logger};
use crate::sstable::format::CompressionType;

/// Configuration for `LsmEngine`.
#[derive(Debug, Clone)]
pub struct LsmOptions {
    /// Mutable MemTable size limit.
    pub write_buffer_size: usize,

    /// Maximum number of mutable + immutable MemTables.
    pub max_write_buffer_number: usize,

    /// Number of L0 files that triggers an L0->L1 compaction.
    pub level0_file_num_compaction_trigger: usize,

    /// L0 file count at which writes are slowed.
    pub level0_slowdown_writes_trigger: usize,

    /// L0 file count at which writes are stalled.
    pub level0_stop_writes_trigger: usize,

    /// Target size for level 1.
    pub max_bytes_for_level_base: u64,

    /// Size ratio between adjacent levels.
    pub max_bytes_for_level_multiplier: u64,

    /// Target SST file size for level 1.
    pub target_file_size_base: u64,

    /// File size growth multiplier per level.
    pub target_file_size_multiplier: u64,

    /// Maximum number of levels.
    pub num_levels: usize,

    /// When an output file's range overlaps more than this many files in the
    /// next-next level, force a new output file.  Bounds the size of future
    /// compactions.
    pub compaction_max_overlap_files: usize,

    /// SST data block size.
    pub block_size: usize,

    /// Restart-point interval inside a data block.
    pub block_restart_interval: usize,

    /// Bloom filter bits per key.
    pub bloom_bits_per_key: usize,

    /// WAL segment size.
    pub wal_segment_size: u64,

    /// Total capacity of the SSTable block cache in bytes.
    pub block_cache_size: usize,

    /// Capacity of the optional cold tier caching blocks as stored on disk
    /// (compressed bytes).  `0` disables it — the default, because the OS
    /// page cache already caches the compressed file contents; enable it for
    /// direct-I/O deployments where the page cache is bypassed.
    pub compressed_block_cache_size: usize,

    /// Compression for SSTable blocks in levels above the bottommost.
    /// LZ4 by default: reads happen on every level, so decompression speed
    /// matters more than ratio.
    pub compression: CompressionType,

    /// Compression for blocks written to the bottommost level.  ZSTD by
    /// default: bottommost blocks are read rarely, so the better ratio pays
    /// for slower decompression.
    pub bottommost_compression: CompressionType,

    /// Optional logger for engine diagnostics.  If `None`, a no-op logger is
    /// used and internal events are silently discarded.
    pub logger: Option<Arc<dyn Logger>>,

    /// Values larger than this are written to the blob log instead of inline.
    /// `0` disables blob separation (all values are inline).
    pub min_blob_value_size: usize,

    /// Maximum size of a single blob file before rotating to a new one.
    pub blob_file_size: u64,

    /// Minimum ratio of live bytes to total bytes that triggers blob GC for an
    /// old blob file.  Values in `(0, 1]`; `0` disables GC.
    pub blob_gc_ratio: f64,

    /// Interval between automatic blob GC passes in milliseconds.  `0` disables
    /// the background worker; explicit `schedule()` calls still run GC.
    pub blob_gc_interval_ms: u64,

    /// Number of threads used for blob GC file scanning.  `0` disables parallel
    /// scanning (single-threaded).  The default is capped at 4 to avoid
    /// overwhelming the I/O subsystem.
    pub blob_gc_threads: usize,

    /// Global garbage ratio above which the background blob GC worker runs
    /// additional passes back-to-back instead of waiting for the regular
    /// interval.  Expressed as garbage bytes / total blob bytes.  `0` disables
    /// forced GC; the regular `blob_gc_interval_ms` interval is still honored.
    pub blob_gc_force_threshold: f64,
}

impl LsmOptions {
    /// Validate options and return an error for impossible combinations.
    pub fn validate(&self) -> crate::Result<()> {
        if self.write_buffer_size == 0 {
            return Err(crate::Error::InvalidArgument(
                "write_buffer_size must be > 0".into(),
            ));
        }
        if self.max_write_buffer_number == 0 {
            return Err(crate::Error::InvalidArgument(
                "max_write_buffer_number must be > 0".into(),
            ));
        }
        if self.num_levels == 0 {
            return Err(crate::Error::InvalidArgument(
                "num_levels must be > 0".into(),
            ));
        }
        if self.level0_stop_writes_trigger <= self.level0_slowdown_writes_trigger {
            return Err(crate::Error::InvalidArgument(
                "level0_stop_writes_trigger must be > level0_slowdown_writes_trigger".into(),
            ));
        }
        if self.block_cache_size == 0 {
            return Err(crate::Error::InvalidArgument(
                "block_cache_size must be > 0".into(),
            ));
        }
        if self.compaction_max_overlap_files == 0 {
            return Err(crate::Error::InvalidArgument(
                "compaction_max_overlap_files must be > 0".into(),
            ));
        }
        if self.blob_gc_ratio < 0.0 || self.blob_gc_ratio > 1.0 {
            return Err(crate::Error::InvalidArgument(
                "blob_gc_ratio must be in [0, 1]".into(),
            ));
        }
        if self.blob_gc_force_threshold < 0.0 || self.blob_gc_force_threshold > 1.0 {
            return Err(crate::Error::InvalidArgument(
                "blob_gc_force_threshold must be in [0, 1]".into(),
            ));
        }
        Ok(())
    }
}

impl Default for LsmOptions {
    fn default() -> Self {
        Self {
            write_buffer_size: 64 * 1024 * 1024,
            max_write_buffer_number: 3,
            level0_file_num_compaction_trigger: 4,
            level0_slowdown_writes_trigger: 12,
            level0_stop_writes_trigger: 20,
            max_bytes_for_level_base: 256 * 1024 * 1024,
            max_bytes_for_level_multiplier: 10,
            target_file_size_base: 64 * 1024 * 1024,
            target_file_size_multiplier: 1,
            num_levels: 7,
            compaction_max_overlap_files: 10,
            block_size: 4 * 1024,
            block_restart_interval: 16,
            bloom_bits_per_key: 10,
            wal_segment_size: 64 * 1024 * 1024,
            block_cache_size: 8 * 1024 * 1024,
            compressed_block_cache_size: 0,
            compression: CompressionType::Lz4,
            bottommost_compression: CompressionType::Zstd,
            logger: None,
            min_blob_value_size: 4 * 1024,
            blob_file_size: 64 * 1024 * 1024,
            blob_gc_ratio: 0.5,
            blob_gc_interval_ms: 30_000,
            blob_gc_threads: std::thread::available_parallelism().map_or(1, |n| n.get().min(4)),
            blob_gc_force_threshold: 0.0,
        }
    }
}

impl LsmOptions {
    /// Return the configured logger, or a shared no-op logger if none was set.
    pub(crate) fn logger(&self) -> Arc<dyn Logger> {
        self.logger.clone().unwrap_or_else(noop_logger)
    }
}
