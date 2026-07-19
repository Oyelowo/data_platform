//! Configuration options for the columnar engine.

/// Configuration for `ColumnarEngineImpl`.
#[derive(Debug, Clone)]
pub struct ColumnarOptions {
    /// Rows to accumulate in memory before flushing a Parquet file.
    pub row_group_size: usize,
    /// Target uncompressed data page size in bytes.
    pub data_page_size: usize,
    /// Column used for partitioning, if any.
    pub partition_column: Option<String>,
    /// Maximum number of small files in a partition before compaction.
    pub max_small_files: usize,
    /// Total size threshold (bytes) triggering compaction.
    pub compaction_threshold_bytes: u64,
    /// Whether to run compaction in a background thread.
    pub background_compaction: bool,
    /// Whether to fsync after every flush.
    pub sync_on_flush: bool,
    /// Target output file size for compaction in bytes. 0 disables splitting.
    pub target_file_size: u64,
}

impl Default for ColumnarOptions {
    fn default() -> Self {
        Self {
            row_group_size: 128_000,
            data_page_size: 1024 * 1024,
            partition_column: None,
            max_small_files: 8,
            compaction_threshold_bytes: 256 * 1024 * 1024,
            background_compaction: true,
            sync_on_flush: true,
            target_file_size: 256 * 1024 * 1024,
        }
    }
}

impl ColumnarOptions {
    /// Validate option values.
    pub fn validate(&self) -> crate::Result<()> {
        if self.row_group_size == 0 {
            return Err(crate::Error::InvalidOption(
                "row_group_size must be > 0".into(),
            ));
        }
        if self.data_page_size == 0 {
            return Err(crate::Error::InvalidOption(
                "data_page_size must be > 0".into(),
            ));
        }
        if self.max_small_files == 0 {
            return Err(crate::Error::InvalidOption(
                "max_small_files must be > 0".into(),
            ));
        }
        Ok(())
    }
}
