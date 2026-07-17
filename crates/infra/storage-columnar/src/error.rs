//! Error types for `storage-columnar`.

/// Result type alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors returned by the columnar engine.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// I/O error from the underlying filesystem or Parquet/Arrow libraries.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// An Arrow operation failed.
    #[error("arrow error: {0}")]
    Arrow(#[from] arrow::error::ArrowError),

    /// A Parquet operation failed.
    #[error("parquet error: {0}")]
    Parquet(#[from] parquet::errors::ParquetError),

    /// Schema-related error.
    #[error("schema error: {0}")]
    Schema(String),

    /// Ingested batch is inconsistent with the table schema.
    #[error("batch error: {0}")]
    Batch(String),

    /// Predicate could not be evaluated.
    #[error("predicate error: {0}")]
    Predicate(String),

    /// Manifest WAL error.
    #[error("manifest wal error: {0}")]
    ManifestWal(String),

    /// Underlying `storage_wal` error.
    #[error("wal error: {0}")]
    Wal(#[from] storage_wal::Error),

    /// JSON serialization/deserialization error.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    /// Option validation error.
    #[error("invalid option: {0}")]
    InvalidOption(String),
}
