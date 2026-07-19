//! Options for `ArtMap`.

use crate::node::MAX_KEY_LEN;

/// Options controlling `ArtMap` behavior and limits.
#[derive(Clone, Debug)]
pub struct ArtMapOptions {
    /// Maximum key length in bytes.
    pub max_key_len: usize,
    /// Maximum value length in bytes.
    pub max_value_len: usize,
    /// Optional hard limit on the number of entries.
    pub max_entries: Option<usize>,
}

impl Default for ArtMapOptions {
    fn default() -> Self {
        Self {
            max_key_len: MAX_KEY_LEN,
            max_value_len: 8 * 1024 * 1024, // 8 MiB
            max_entries: None,
        }
    }
}
