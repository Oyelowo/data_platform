//! Error types for `storage-time-series`.

use std::fmt;

/// Result type alias for `storage-time-series` operations.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors returned by the time-series engine.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// An I/O operation failed.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// Data on disk is corrupted or unreadable.
    #[error("corruption: {0}")]
    Corruption(String),

    /// A configuration value is invalid.
    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    /// The requested operation is not supported with the current configuration.
    #[error("unsupported: {0}")]
    Unsupported(&'static str),

    /// A requested key, series, or sample was not found.
    #[error("not found: {0}")]
    NotFound(String),

    /// The underlying WAL reported an error.
    #[error("wal error: {0}")]
    Wal(#[from] storage_wal::Error),

    /// A compression/decompression operation failed.
    #[error("compression error: {0}")]
    Compression(String),

    /// A transaction was used after it was already committed or rolled back.
    #[error("inactive transaction")]
    InactiveTransaction,

    /// A write was attempted on a read-only transaction.
    #[error("read-only transaction")]
    ReadOnlyTransaction,

    /// A concurrency or isolation conflict occurred.
    #[error("conflict: {0}")]
    Conflict(String),
}

impl Error {
    /// Create a corruption error from a displayable message.
    pub fn corruption(msg: impl fmt::Display) -> Self {
        Self::Corruption(msg.to_string())
    }

    /// Create a not-found error.
    pub fn not_found(msg: impl fmt::Display) -> Self {
        Self::NotFound(msg.to_string())
    }

    /// Create an invalid-argument error.
    pub fn invalid_argument(msg: impl fmt::Display) -> Self {
        Self::InvalidArgument(msg.to_string())
    }

    /// Create a compression error.
    pub fn compression(msg: impl fmt::Display) -> Self {
        Self::Compression(msg.to_string())
    }
}

impl From<storage_compression::Error> for Error {
    fn from(e: storage_compression::Error) -> Self {
        Error::Compression(e.to_string())
    }
}
