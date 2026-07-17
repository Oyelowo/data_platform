//! Core storage engine trait.

use bytes::Bytes;

use crate::cursor::Cursor;
use crate::error::Result;
use crate::options::TxnOptions;
use crate::stats::EngineStats;
use crate::transaction::Transaction;

/// A synchronous, thread-safe storage engine.
///
/// The API is byte-oriented: keys and values are opaque byte sequences. This
/// is the correct abstraction for storage engines because higher-level types
/// serialize to bytes before reaching the engine.
pub trait Engine: Send + Sync + 'static {
    /// Error type returned by this engine.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Transaction type.
    type Transaction: Transaction<Error = Self::Error>;

    /// Cursor type returned by scans.
    type Cursor: Cursor<Error = Self::Error>;

    /// Human-readable engine name, used for metrics and logging.
    fn name(&self) -> &'static str;

    /// Begin a new transaction.
    fn begin(&self, opts: TxnOptions) -> Result<Self::Transaction, Self::Error>;

    /// Read a single key outside of a transaction.
    fn get(&self, key: &[u8]) -> Result<Option<Bytes>, Self::Error>;

    /// Scan keys in the half-open interval `[start, end)` in ascending order.
    ///
    /// `None` for `start` means "from the first key"; `None` for `end` means
    /// "to the last key".
    fn scan(&self, start: Option<&[u8]>, end: Option<&[u8]>) -> Result<Self::Cursor, Self::Error>;

    /// Return engine statistics.
    fn stats(&self) -> Result<EngineStats, Self::Error>;

    /// Flush all durable state to stable storage.
    ///
    /// For purely in-memory engines this is a no-op.
    fn sync(&self) -> Result<(), Self::Error>;
}
