//! Error types for `storage-kv`.

/// Result alias used throughout `storage-kv`.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors returned by the LSM engine.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("wal error: {0}")]
    Wal(#[from] storage_wal::Error),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("manifest error: {0}")]
    Manifest(String),

    #[error("corruption: {0}")]
    Corruption(String),

    #[error("sstable error: {0}")]
    Sstable(String),

    #[error("blob error: {0}")]
    Blob(String),

    #[error("transaction already committed or rolled back")]
    TxnFinished,

    #[error("read-only transaction cannot write")]
    ReadOnlyTxn,

    #[error("database closed")]
    Closed,

    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    #[error("database busy: {0}")]
    Busy(String),
}
