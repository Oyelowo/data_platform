//! Error types for `storage-btree`.

/// Result alias used throughout `storage-btree`.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors returned by the B+ tree engine.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// An I/O operation failed.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// The WAL reported an error.
    #[error("wal error: {0}")]
    Wal(#[from] storage_wal::Error),

    /// Data on disk is corrupted or unreadable.
    #[error("corruption: {0}")]
    Corruption(String),

    /// A requested resource was not found.
    #[error("not found: {0}")]
    NotFound(String),

    /// An operation is not supported by this engine.
    #[error("unsupported: {0}")]
    Unsupported(&'static str),

    /// An argument was invalid.
    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    /// A key or value exceeded the engine's size limits.
    #[error("out of bounds: {kind} limit is {limit} bytes, got {got}")]
    OutOfBounds {
        /// Which limit was exceeded.
        kind: BoundKind,
        /// The configured limit.
        limit: usize,
        /// The supplied size.
        got: usize,
    },

    /// The transaction has already been committed or rolled back.
    #[error("transaction finished")]
    TxnFinished,

    /// A read-only transaction attempted a write.
    #[error("read-only transaction cannot write")]
    ReadOnlyTxn,

    /// A page does not have enough free space for the requested record.
    #[error("page full")]
    PageFull,
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

impl std::fmt::Display for BoundKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BoundKind::Key => write!(f, "key"),
            BoundKind::Value => write!(f, "value"),
            BoundKind::Batch => write!(f, "batch"),
        }
    }
}
