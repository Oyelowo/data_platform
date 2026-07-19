//! Ordered iteration over feature keys for the `Engine` trait.

use std::collections::BTreeMap;

use bytes::Bytes;
use storage_traits::Cursor;

/// Cursor over a snapshot of feature keys and values.
pub struct GeoCursor {
    items: Vec<(Bytes, Bytes)>,
    position: usize,
}

impl GeoCursor {
    /// Create a cursor from a sorted map.
    pub fn new(map: BTreeMap<Vec<u8>, Vec<u8>>) -> Self {
        let items: Vec<_> = map
            .into_iter()
            .map(|(k, v)| (Bytes::from(k), Bytes::from(v)))
            .collect();
        Self {
            items,
            position: 0,
        }
    }
}

impl Iterator for GeoCursor {
    type Item = crate::Result<(Bytes, Bytes)>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.position < self.items.len() {
            let (k, v) = self.items[self.position].clone();
            self.position += 1;
            Some(Ok((k, v)))
        } else {
            None
        }
    }
}

impl Cursor for GeoCursor {
    type Error = crate::Error;

    fn seek(&mut self, target: &[u8]) -> crate::Result<()> {
        self.position = self
            .items
            .binary_search_by(|(k, _)| k.as_ref().cmp(target))
            .unwrap_or_else(|i| i);
        Ok(())
    }
}
