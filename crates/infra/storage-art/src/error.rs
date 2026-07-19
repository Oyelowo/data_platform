//! Error types for `storage-art`.

/// Result type alias for `storage-art` operations.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors that can occur when operating on an `ArtMap`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Key exceeds the configured maximum length.
    #[error("key length {len} exceeds maximum {max}")]
    KeyTooLong {
        /// Actual key length.
        len: usize,
        /// Configured maximum.
        max: usize,
    },

    /// Value exceeds the configured maximum length.
    #[error("value length {len} exceeds maximum {max}")]
    ValueTooLong {
        /// Actual value length.
        len: usize,
        /// Configured maximum.
        max: usize,
    },

    /// Map has reached its configured entry limit.
    #[error("map entry limit {0} reached")]
    EntryLimitReached(usize),
}
