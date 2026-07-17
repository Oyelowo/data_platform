//! Configuration options for the Bw-Tree engine.

/// Options used when opening a [`BwTreeEngine`](crate::BwTreeEngine).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BwTreeOptions {
    /// Size of a logical page in bytes.
    ///
    /// This is used as the byte threshold for splits and merges. The default is
    /// 4096.
    pub page_size: usize,

    /// Maximum size of an inline value in bytes.
    ///
    /// Values larger than this are stored in overflow pages. The default is
    /// 1 MiB so the shared conformance tests can store large values without
    /// requiring overflow support in the first version.
    pub max_inline_value_size: usize,

    /// Maximum length of a leaf delta chain before consolidation.
    ///
    /// The default is 24, matching the OpenBw-Tree baseline.
    pub max_delta_chain_len_leaf: usize,

    /// Maximum length of an inner delta chain before consolidation.
    ///
    /// Inner nodes usually have shorter chains because separators are larger.
    /// The default is 8.
    pub max_delta_chain_len_inner: usize,

    /// Minimum fill ratio for a node after deletion, expressed as a percentage.
    ///
    /// The default is 50. If a node falls below this ratio after a delete, the
    /// engine tries to merge it with its left sibling.
    pub min_fill_percent: usize,
}

impl BwTreeOptions {
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
        if self.max_delta_chain_len_leaf == 0 || self.max_delta_chain_len_inner == 0 {
            return Err(crate::Error::InvalidArgument(
                "delta chain length thresholds must be non-zero".into(),
            ));
        }
        Ok(Self {
            page_size: self.page_size,
            max_inline_value_size: self.max_inline_value_size,
            max_delta_chain_len_leaf: self.max_delta_chain_len_leaf,
            max_delta_chain_len_inner: self.max_delta_chain_len_inner,
            min_fill_percent: self.min_fill_percent,
        })
    }

    /// Return the byte threshold at which a node is considered full.
    pub(crate) fn node_size_threshold(&self) -> usize {
        self.page_size
    }

    /// Return the byte threshold below which a node is considered underfull.
    pub(crate) fn min_node_size(&self) -> usize {
        self.page_size * self.min_fill_percent / 100
    }
}

impl Default for BwTreeOptions {
    fn default() -> Self {
        Self {
            page_size: 4096,
            max_inline_value_size: 1024 * 1024,
            max_delta_chain_len_leaf: 24,
            max_delta_chain_len_inner: 8,
            min_fill_percent: 50,
        }
    }
}
