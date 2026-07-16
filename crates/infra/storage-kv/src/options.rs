//! Configuration options for the LSM engine.

/// Configuration for `LsmEngine`.
#[derive(Debug, Clone, Copy)]
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

    /// SST data block size.
    pub block_size: usize,

    /// Restart-point interval inside a data block.
    pub block_restart_interval: usize,

    /// Bloom filter bits per key.
    pub bloom_bits_per_key: usize,

    /// WAL segment size.
    pub wal_segment_size: u64,
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
            block_size: 4 * 1024,
            block_restart_interval: 16,
            bloom_bits_per_key: 10,
            wal_segment_size: 64 * 1024 * 1024,
        }
    }
}
