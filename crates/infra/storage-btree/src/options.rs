//! Configuration options for the B+ tree engine.

/// Options used when opening a [`BtreeEngine`](crate::BtreeEngine).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BtreeOptions {
    /// Size of each on-disk page in bytes.
    ///
    /// Must be a power of two and at least 512. The default is 4096.
    pub page_size: usize,

    /// Maximum size of an inline value in bytes.
    ///
    /// Values larger than this are stored in overflow pages. The default is
    /// one quarter of the page size.
    pub max_inline_value_size: usize,

    /// Minimum fill ratio for a node after deletion, expressed as a percentage.
    ///
    /// The default is 50. If a node falls below this ratio after a delete, the
    /// engine tries to redistribute entries from a sibling; if redistribution is
    /// not possible, the node is merged.
    pub min_fill_percent: usize,

    /// Maximum size of the in-memory page cache in bytes.
    ///
    /// A value of zero means unlimited. The default is 64 MiB.
    pub cache_size: usize,
}

impl BtreeOptions {
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
        Ok(Self {
            page_size: self.page_size,
            max_inline_value_size: self.max_inline_value_size.min(self.page_size / 4),
            min_fill_percent: self.min_fill_percent,
            cache_size: self.cache_size,
        })
    }
}

impl Default for BtreeOptions {
    fn default() -> Self {
        Self {
            page_size: 4096,
            max_inline_value_size: 1024,
            min_fill_percent: 50,
            cache_size: 64 * 1024 * 1024,
        }
    }
}
