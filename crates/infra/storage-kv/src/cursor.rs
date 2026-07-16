//! Cursor for range scans over the LSM engine.

use bytes::Bytes;

use crate::engine::LsmEngineInner;
use crate::Result;
use crate::SequenceNumber;

/// Cursor over a key range.
pub struct LsmCursor {
    entries: Vec<(Bytes, Bytes)>,
    position: usize,
}

impl LsmCursor {
    pub fn new(
        inner: LsmEngineInner,
        start: Option<Vec<u8>>,
        end: Option<Vec<u8>>,
        snapshot: SequenceNumber,
    ) -> Self {
        let entries = inner.scan_entries(start.as_deref(), end.as_deref(), snapshot);
        Self {
            entries,
            position: 0,
        }
    }

    fn current(&self) -> Option<(Bytes, Bytes)> {
        self.entries.get(self.position).cloned()
    }
}

impl Iterator for LsmCursor {
    type Item = Result<(Bytes, Bytes)>;

    fn next(&mut self) -> Option<Self::Item> {
        let item = self.current();
        if item.is_some() {
            self.position += 1;
        }
        item.map(Ok)
    }
}

impl storage_traits::Cursor for LsmCursor {
    type Error = crate::Error;

    fn seek(&mut self, key: &[u8]) -> Result<()> {
        self.position = self
            .entries
            .partition_point(|(k, _)| k.as_ref() < key);
        Ok(())
    }

    fn next_batch(&mut self, limit: usize) -> Result<Vec<(Bytes, Bytes)>> {
        let end = (self.position + limit).min(self.entries.len());
        let out = self.entries[self.position..end].to_vec();
        self.position = end;
        Ok(out)
    }
}
