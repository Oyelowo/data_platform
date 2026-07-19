//! Ordered iteration over stored documents.

use std::collections::BTreeMap;

use bytes::Bytes;
use storage_traits::Cursor;

/// Cursor over stored documents.
#[derive(Debug)]
pub struct SearchCursor {
    items: Vec<(Vec<u8>, Vec<u8>)>,
    pos: usize,
}

impl SearchCursor {
    /// Create a cursor from a map of doc_id -> encoded document.
    pub fn from_map(map: BTreeMap<Vec<u8>, Vec<u8>>) -> Self {
        Self {
            items: map.into_iter().collect(),
            pos: 0,
        }
    }
}

impl Iterator for SearchCursor {
    type Item = std::result::Result<(Bytes, Bytes), crate::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.items.len() {
            return None;
        }
        let (k, v) = self.items.get(self.pos)?;
        self.pos += 1;
        Some(Ok((Bytes::from(k.clone()), Bytes::from(v.clone()))))
    }
}

impl Cursor for SearchCursor {
    type Error = crate::Error;

    fn seek(&mut self, target: &[u8]) -> std::result::Result<(), Self::Error> {
        self.pos = self
            .items
            .binary_search_by(|(k, _)| k.as_slice().cmp(target))
            .unwrap_or_else(|i| i);
        Ok(())
    }
}
