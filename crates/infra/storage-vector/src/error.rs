//! Error types for `storage-vector`.

use std::fmt;

/// Result type alias for `storage-vector` operations.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors returned by the vector engine.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// An I/O operation failed.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// A vector dimension does not match the engine's configured dimension.
    #[error("dimension mismatch: expected {expected}, got {got}")]
    DimensionMismatch {
        /// Expected dimension.
        expected: usize,
        /// Supplied dimension.
        got: usize,
    },

    /// A requested key or vector was not found.
    #[error("not found: {0}")]
    NotFound(String),

    /// Data on disk is corrupted or unreadable.
    #[error("corruption: {0}")]
    Corruption(String),

    /// A configuration value is invalid.
    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    /// The requested operation is not supported with the current configuration.
    #[error("unsupported: {0}")]
    Unsupported(&'static str),

    /// A concurrency or isolation conflict occurred.
    #[error("conflict: {0}")]
    Conflict(String),

    /// The underlying WAL reported an error.
    #[error("wal error: {0}")]
    Wal(#[from] storage_wal::Error),

    /// A transaction was used after it was already committed or rolled back.
    #[error("inactive transaction")]
    InactiveTransaction,

    /// A write was attempted on a read-only transaction.
    #[error("read-only transaction")]
    ReadOnlyTransaction,

    /// The in-memory index is not built yet.
    #[error("index not ready")]
    IndexNotReady,
}

impl Error {
    /// Create a dimension mismatch error.
    pub fn dimension_mismatch(expected: usize, got: usize) -> Self {
        Self::DimensionMismatch { expected, got }
    }

    /// Create a corruption error from a displayable message.
    pub fn corruption(msg: impl fmt::Display) -> Self {
        Self::Corruption(msg.to_string())
    }

    /// Create a not-found error.
    pub fn not_found(msg: impl fmt::Display) -> Self {
        Self::NotFound(msg.to_string())
    }
}
