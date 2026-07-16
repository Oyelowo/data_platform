//! Transaction trait.

use bytes::Bytes;

use crate::cursor::Cursor;
use crate::error::Result;
use crate::options::IsolationLevel;

/// A storage transaction.
///
/// Implementations define their own concurrency and isolation mechanism. The
/// trait only expresses the contract callers can rely on.
pub trait Transaction: Sized + Send {
    /// Error type, shared with the parent engine.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Read a single key.
    fn get(&self, key: &[u8]) -> Result<Option<Bytes>, Self::Error>;

    /// Write or overwrite a key.
    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<(), Self::Error>;

    /// Remove a key. Removing a missing key is not an error.
    fn delete(&mut self, key: &[u8]) -> Result<(), Self::Error>;

    /// Scan keys in the half-open interval `[start, end)` in ascending order
    /// within this transaction's view.
    fn scan(
        &self,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
    ) -> Result<impl Cursor<Error = Self::Error>, Self::Error>;

    /// Commit all writes in the transaction.
    fn commit(self) -> Result<(), Self::Error>;

    /// Abort the transaction and discard all writes.
    fn rollback(self) -> Result<(), Self::Error>;

    /// Change the isolation level for subsequent operations.
    ///
    /// Engines that cannot change isolation mid-transaction may return
    /// [`Error::Unsupported`](crate::Error::Unsupported).
    fn set_isolation(&mut self, level: IsolationLevel) -> Result<(), Self::Error>;
}
