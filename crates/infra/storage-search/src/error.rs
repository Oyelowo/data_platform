//! Error types for `storage-search`.

use std::fmt;

/// Result type alias for `storage-search` operations.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors returned by the search engine.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// An I/O operation failed.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// A requested document or field was not found.
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

    /// A query string could not be parsed.
    #[error("parse error: {0}")]
    ParseError(String),

    /// A document was rejected because a key or field was too large.
    #[error("out of bounds: {kind} limit {limit}, got {got}")]
    OutOfBounds {
        /// Kind of bound that was exceeded.
        kind: &'static str,
        /// Configured limit.
        limit: usize,
        /// Actual value.
        got: usize,
    },
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

    /// Create a parse error.
    pub fn parse(msg: impl fmt::Display) -> Self {
        Self::ParseError(msg.to_string())
    }
}

impl From<crate::query::parser::ParseError> for Error {
    fn from(e: crate::query::parser::ParseError) -> Self {
        Error::ParseError(e.0)
    }
}
