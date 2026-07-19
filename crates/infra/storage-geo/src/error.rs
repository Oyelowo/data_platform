//! Error types for `storage-geo`.

use storage_traits::Error as TraitError;

/// Result type alias used throughout this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors returned by the geospatial storage engine.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// An I/O operation failed.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// A supplied argument was invalid.
    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    /// Data on disk is corrupted or otherwise unreadable.
    #[error("corruption detected: {0}")]
    Corruption(String),

    /// A requested feature or property was not found.
    #[error("not found: {0}")]
    NotFound(String),

    /// A transaction was used after it was already committed or rolled back.
    #[error("transaction is no longer active")]
    InactiveTransaction,

    /// A write was attempted on a read-only transaction.
    #[error("cannot write in a read-only transaction")]
    ReadOnlyTransaction,

    /// An operation is not supported by this engine.
    #[error("operation not supported: {0}")]
    Unsupported(&'static str),

    /// A key or value exceeded the engine's size limits.
    #[error("{kind} out of bounds: limit {limit}, got {got}")]
    OutOfBounds {
        /// Which limit was exceeded.
        kind: &'static str,
        /// The configured limit.
        limit: usize,
        /// The supplied size.
        got: usize,
    },

    /// A geometry failed validation.
    #[error("invalid geometry: {0}")]
    InvalidGeometry(String),

    /// Well-Known Binary encoding or decoding failed.
    #[error("wkb error: {0}")]
    Wkb(String),

    /// Property map JSON encoding or decoding failed.
    #[error("property encoding error: {0}")]
    PropertyEncoding(String),

    /// A concurrency or isolation conflict occurred.
    #[error("conflict: {0}")]
    Conflict(String),

    /// The write-ahead log returned an error.
    #[error("wal error: {0}")]
    Wal(#[from] storage_wal::Error),
}

impl Error {
    /// Create an `InvalidArgument` error.
    pub fn invalid_argument<S: Into<String>>(msg: S) -> Self {
        Self::InvalidArgument(msg.into())
    }

    /// Create a `Corruption` error.
    pub fn corruption<S: Into<String>>(msg: S) -> Self {
        Self::Corruption(msg.into())
    }

    /// Create a `NotFound` error.
    pub fn not_found<S: Into<String>>(msg: S) -> Self {
        Self::NotFound(msg.into())
    }

    /// Create an `InvalidGeometry` error.
    pub fn invalid_geometry<S: Into<String>>(msg: S) -> Self {
        Self::InvalidGeometry(msg.into())
    }

    /// Create a `Wkb` error.
    pub fn wkb<S: Into<String>>(msg: S) -> Self {
        Self::Wkb(msg.into())
    }

    /// Create a `PropertyEncoding` error.
    pub fn property_encoding<S: Into<String>>(msg: S) -> Self {
        Self::PropertyEncoding(msg.into())
    }
}

impl From<TraitError> for Error {
    fn from(e: TraitError) -> Self {
        match e {
            TraitError::Io(io) => Error::Io(io),
            TraitError::OutOfBounds { kind, limit, got } => {
                let kind_str = match kind {
                    storage_traits::BoundKind::Key => "key",
                    storage_traits::BoundKind::Value => "value",
                    storage_traits::BoundKind::Batch => "batch",
                };
                Error::OutOfBounds {
                    kind: kind_str,
                    limit,
                    got,
                }
            }
            TraitError::InactiveTransaction => Error::InactiveTransaction,
            TraitError::ReadOnlyTransaction => Error::ReadOnlyTransaction,
            TraitError::Unsupported(msg) => Error::Unsupported(msg),
            TraitError::Corruption(msg) => Error::Corruption(msg),
            TraitError::NotFound(msg) => Error::NotFound(msg),
            TraitError::Conflict(msg) => Error::Conflict(msg),
            _ => Error::InvalidArgument("unknown trait error".into()),
        }
    }
}
