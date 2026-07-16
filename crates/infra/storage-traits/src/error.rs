//! Typed errors for storage operations.

use std::fmt;

/// The result type used throughout the storage layer.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// A storage operation failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// An I/O operation failed.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// A key or value exceeded the engine's size limits.
    #[error("value out of bounds: {kind} limit is {limit} bytes, got {got}")]
    OutOfBounds {
        /// Which limit was exceeded.
        kind: BoundKind,
        /// The configured limit.
        limit: usize,
        /// The supplied size.
        got: usize,
    },

    /// A transaction was used after it was already committed or rolled back.
    #[error("transaction is no longer active")]
    InactiveTransaction,

    /// A write was attempted on a read-only transaction.
    #[error("cannot write in a read-only transaction")]
    ReadOnlyTransaction,

    /// An operation is not supported by this engine.
    #[error("operation not supported: {0}")]
    Unsupported(&'static str),

    /// Data on disk is corrupted or otherwise unreadable.
    #[error("corruption detected: {0}")]
    Corruption(String),

    /// A requested resource was not found.
    #[error("not found: {0}")]
    NotFound(String),

    /// A concurrency or isolation conflict occurred.
    #[error("conflict: {0}")]
    Conflict(String),
}

/// The kind of size bound that was violated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoundKind {
    /// The maximum allowed key size.
    Key,
    /// The maximum allowed inline value size.
    Value,
    /// The maximum allowed batch size.
    Batch,
}

impl fmt::Display for BoundKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BoundKind::Key => write!(f, "key"),
            BoundKind::Value => write!(f, "value"),
            BoundKind::Batch => write!(f, "batch"),
        }
    }
}
