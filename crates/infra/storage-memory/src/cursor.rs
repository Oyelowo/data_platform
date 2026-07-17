//! In-memory cursor implementation.

use bytes::Bytes;

use storage_traits::{Cursor, Error, Result};

/// Cursor over a snapshot of the in-memory engine.
pub struct MemoryCursor {
    inner: Option<MemoryCursorInner>,
}

struct MemoryCursorInner {
    /// Buffered entries, sorted ascending.
    buffer: Vec<(Bytes, Bytes)>,
    position: usize,
}

impl MemoryCursor {
    /// Create a cursor from an already-filtered, sorted buffer.
    pub(crate) fn from_snapshot(buffer: Vec<(Bytes, Bytes)>) -> Self {
        let inner = MemoryCursorInner {
            buffer,
            position: 0,
        };
        Self { inner: Some(inner) }
    }

    fn inner(&mut self) -> Result<&mut MemoryCursorInner> {
        self.inner.as_mut().ok_or(Error::InactiveTransaction)
    }
}

impl Iterator for MemoryCursor {
    type Item = Result<(Bytes, Bytes)>;

    fn next(&mut self) -> Option<Self::Item> {
        let inner = self.inner.as_mut()?;
        if inner.position < inner.buffer.len() {
            let item = inner.buffer[inner.position].clone();
            inner.position += 1;
            Some(Ok(item))
        } else {
            None
        }
    }
}

impl Cursor for MemoryCursor {
    type Error = Error;

    fn seek(&mut self, target: &[u8]) -> Result<()> {
        let inner = self.inner()?;
        inner.position = inner.buffer.partition_point(|(k, _)| k.as_ref() < target);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_empty() {
        let mut cursor = MemoryCursor::from_snapshot(Vec::new());
        assert!(cursor.next().is_none());
    }
}
