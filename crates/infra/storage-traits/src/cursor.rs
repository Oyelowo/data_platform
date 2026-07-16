//! Ordered iteration over key-value pairs.

use bytes::Bytes;

/// A synchronous cursor over an ordered key-value stream.
///
/// Implementations must return keys in strictly ascending byte order.
pub trait Cursor: Iterator<Item = std::result::Result<(Bytes, Bytes), Self::Error>> {
    /// Error type returned by the cursor.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Reposition the cursor on the first key that is greater than or equal to
    /// `target`. If no such key exists, the cursor becomes exhausted.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying engine fails during the seek.
    fn seek(&mut self, target: &[u8]) -> std::result::Result<(), Self::Error>;

    /// Pull the next `n` entries into a vector.
    ///
    /// The returned vector may contain fewer than `n` items if the cursor is
    /// exhausted. This default implementation bounds allocation to avoid
    /// unbounded memory growth on malicious ranges.
    fn next_batch(&mut self, n: usize) -> std::result::Result<Vec<(Bytes, Bytes)>, Self::Error> {
        let mut out = Vec::with_capacity(n.min(1024));
        for _ in 0..n {
            match self.next() {
                Some(Ok(kv)) => out.push(kv),
                Some(Err(e)) => return Err(e),
                None => break,
            }
        }
        Ok(out)
    }
}

/// An asynchronous cursor over an ordered key-value stream.
#[cfg(feature = "async")]
pub trait AsyncCursor: futures_core::Stream<Item = crate::error::Result<(Bytes, Bytes)>> {
    /// Error type returned by the cursor.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Reposition the cursor on the first key >= `target`.
    fn seek(self, target: Bytes) -> impl std::future::Future<Output = crate::error::Result<Self>>
    where
        Self: Sized;
}
