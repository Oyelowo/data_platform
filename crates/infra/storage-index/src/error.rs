//! Error types for `storage-index`.

/// Result alias used throughout `storage-index`.
pub type Result<T, E> = std::result::Result<T, Error<E>>;

/// Errors returned by the index engine.
#[derive(Debug, thiserror::Error)]
pub enum Error<Source: std::error::Error + Send + Sync + 'static> {
    /// An error propagated from the underlying storage engine.
    #[error("underlying engine error: {0}")]
    Engine(#[source] Source),

    /// An index with this name already exists.
    #[error("index name `{0}` already exists")]
    DuplicateName(String),

    /// The requested index does not exist.
    #[error("index `{0}` not found")]
    UnknownIndex(String),

    /// The durable catalog is malformed.
    #[error("corrupt catalog: {0}")]
    CorruptCatalog(String),

    /// A value could not be parsed as a [`Record`](crate::Record).
    #[error("invalid record: {0}")]
    InvalidRecord(String),

    /// An argument violated a precondition.
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
}

impl<Source: std::error::Error + Send + Sync + 'static> From<Source> for Error<Source> {
    fn from(err: Source) -> Self {
        Error::Engine(err)
    }
}
