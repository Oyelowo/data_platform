//! In-memory cursor implementation.

use bytes::Bytes;
use crossbeam_skiplist::SkipMap;
use std::ops::Bound;
use std::sync::Arc;

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
    /// Create a new cursor over `[start, end)`.
    pub(crate) fn new(
        data: Arc<SkipMap<Bytes, Bytes>>,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
    ) -> Self {
        // Materialize a snapshot into a vector. This gives us a stable,
        // seekable cursor without holding a long-lived reference into the
        // lock-free map. The cost is O(N) memory for the scanned range.
        let lower: Bound<&[u8]> = start.map_or(Bound::Unbounded, Bound::Included);
        let upper: Bound<&[u8]> = end.map_or(Bound::Unbounded, Bound::Excluded);

        let mut buffer: Vec<(Bytes, Bytes)> = data
            .range::<[u8], _>((lower, upper))
            .map(|e| (e.key().clone(), e.value().clone()))
            .collect();
        buffer.sort_by(|a, b| a.0.cmp(&b.0));

        let inner = MemoryCursorInner {
            buffer,
            position: 0,
        };

        Self {
            inner: Some(inner),
        }
    }

    /// Create a cursor from an already-filtered, sorted buffer.
    pub(crate) fn from_snapshot(buffer: Vec<(Bytes, Bytes)>) -> Self {
        let inner = MemoryCursorInner {
            buffer,
            position: 0,
        };
        Self {
            inner: Some(inner),
        }
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
        inner.position = inner
            .buffer
            .partition_point(|(k, _)| k.as_ref() < target);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_empty() {
        let data = Arc::new(SkipMap::new());
        let mut cursor = MemoryCursor::new(data, None, None);
        assert!(cursor.next().is_none());
    }
}
