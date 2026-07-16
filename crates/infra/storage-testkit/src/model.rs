//! In-memory reference model for verifying storage engine behavior.

use bytes::Bytes;
use std::collections::BTreeMap;

/// A simple B-tree map oracle.
#[derive(Clone, Debug, Default)]
pub struct Model {
    data: BTreeMap<Vec<u8>, Vec<u8>>,
}

impl Model {
    /// Create an empty model.
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply a put.
    pub fn put(&mut self, key: Bytes, value: Bytes) {
        self.data.insert(key.to_vec(), value.to_vec());
    }

    /// Apply a delete.
    pub fn delete(&mut self, key: &[u8]) {
        self.data.remove(key);
    }

    /// Get a value.
    pub fn get(&self, key: &[u8]) -> Option<Bytes> {
        self.data.get(key).map(|v| Bytes::copy_from_slice(v))
    }

    /// Scan a half-open range `[start, end)`.
    pub fn scan(&self, start: Option<&[u8]>, end: Option<&[u8]>) -> Vec<(Bytes, Bytes)> {
        let start_bound = start.map_or(std::ops::Bound::Unbounded, |s| {
            std::ops::Bound::Included(s.to_vec())
        });
        let end_bound = end.map_or(std::ops::Bound::Unbounded, |e| {
            std::ops::Bound::Excluded(e.to_vec())
        });
        self.data
            .range((start_bound, end_bound))
            .map(|(k, v)| (Bytes::copy_from_slice(k), Bytes::copy_from_slice(v)))
            .collect()
    }

    /// Apply a batch of operations.
    pub fn apply(&mut self, ops: &[Op]) {
        for op in ops {
            match op {
                Op::Put { key, value } => self.put(key.clone(), value.clone()),
                Op::Delete { key } => self.delete(key),
            }
        }
    }
}

/// A single operation against the model or an engine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    /// Insert or overwrite a key.
    Put {
        /// Key to write.
        key: Bytes,
        /// Value to write.
        value: Bytes,
    },
    /// Remove a key.
    Delete {
        /// Key to remove.
        key: Bytes,
    },
}
