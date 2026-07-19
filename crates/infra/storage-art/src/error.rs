//! Error types for `storage-art`.

/// Result type alias for `storage-art` operations.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors that can occur when operating on an `ArtMap` or `ArtEngine`.
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

    /// Snapshot or metadata data is corrupted or malformed.
    #[error("corruption detected: {0}")]
    Corruption(String),

    /// An I/O operation failed.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// The underlying write-ahead log returned an error.
    #[error("wal error: {0}")]
    Wal(#[from] storage_wal::Error),

    /// An invalid argument was supplied.
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
}

impl Error {
    /// Helper for building `InvalidArgument` errors.
    pub fn invalid_argument<S: Into<String>>(msg: S) -> Self {
        Self::InvalidArgument(msg.into())
    }
}
