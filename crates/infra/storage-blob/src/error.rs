//! Error types for `storage-blob`.

use std::path::PathBuf;

/// Result type alias for `storage-blob`.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors returned by the blob store.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// I/O error from the underlying filesystem.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// Object not found.
    #[error("object not found: {0:?}")]
    NotFound(Vec<u8>),

    /// Corrupt or unrecognised volume record.
    #[error("corrupt record in volume {volume} at offset {offset}: {message}")]
    CorruptRecord {
        /// Volume file number.
        volume: u64,
        /// Byte offset of the record header.
        offset: u64,
        /// Human-readable description.
        message: String,
    },

    /// Index WAL error.
    #[error("index wal error: {0}")]
    IndexWal(String),

    /// Underlying write-ahead log error.
    #[error("wal error: {0}")]
    Wal(#[from] storage_wal::Error),

    /// Invalid path.
    #[error("invalid blob store path: {0}")]
    InvalidPath(PathBuf),

    /// Invalid option value.
    #[error("invalid option: {0}")]
    InvalidOption(String),
}
