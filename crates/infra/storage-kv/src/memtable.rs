//! In-memory write buffer backed by `storage-skipmap`.

use bytes::Bytes;

use crate::internal_key::{build_internal_key, compare_internal_keys, extract_user_key, parse_internal_key, ValueType};
use crate::SequenceNumber;

/// A mutable in-memory table of recent writes.
pub struct MemTable {
    map: storage_skipmap::SkipMap<Vec<u8>, Bytes>,
    approximate_size: std::sync::atomic::AtomicUsize,
}

impl MemTable {
    /// Create a new empty MemTable.
    pub fn new() -> Self {
        Self {
            map: storage_skipmap::SkipMap::new(),
            approximate_size: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// Insert a key/value pair with the given sequence number.
    pub fn put(&self, key: &[u8], seq: SequenceNumber, value: &[u8]) {
        let ikey = build_internal_key(key, seq, ValueType::Value);
        self.approximate_size
            .fetch_add(ikey.len() + value.len(), std::sync::atomic::Ordering::Relaxed);
        self.map.insert(ikey, Bytes::copy_from_slice(value));
    }

    /// Insert a deletion tombstone with the given sequence number.
    pub fn delete(&self, key: &[u8], seq: SequenceNumber) {
        let ikey = build_internal_key(key, seq, ValueType::Deletion);
        self.approximate_size
            .fetch_add(ikey.len(), std::sync::atomic::Ordering::Relaxed);
        self.map.insert(ikey, Bytes::new());
    }

    /// Look up the newest visible value for `key` at or before `snapshot_seq`.
    pub fn get(&self, key: &[u8], snapshot_seq: SequenceNumber) -> Option<Option<Bytes>> {
        let mut best: Option<(SequenceNumber, Option<Option<Bytes>>)> = None;
        for (ikey, value) in self.map.iter() {
            if extract_user_key(&ikey) != key {
                continue;
            }
            let (seq, ty) = parse_internal_key(&ikey)?;
            if seq > snapshot_seq {
                continue;
            }
            let candidate = match ty {
                ValueType::Value => Some(Some(value)),
                ValueType::Deletion => Some(None),
            };
            if best.as_ref().map(|(s, _)| seq > *s).unwrap_or(true) {
                best = Some((seq, candidate));
            }
        }
        best.map(|(_, v)| v).unwrap_or(None)
    }

    /// Approximate byte size of the MemTable.
    pub fn approximate_size(&self) -> usize {
        self.approximate_size.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Return all entries in ascending internal-key order.
    pub fn iter(&self) -> Vec<(Vec<u8>, Bytes)> {
        let mut entries = self.map.iter();
        entries.sort_by(|a, b| compare_internal_keys(&a.0, &b.0));
        entries
    }
}

impl Default for MemTable {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_and_get() {
        let mt = MemTable::new();
        mt.put(b"a", 5, b"v2");
        mt.put(b"a", 10, b"v1");
        assert_eq!(mt.get(b"a", 10), Some(Some(Bytes::from_static(b"v1"))));
        assert_eq!(mt.get(b"a", 7), Some(Some(Bytes::from_static(b"v2"))));
        assert_eq!(mt.get(b"a", 4), None);
    }

    #[test]
    fn delete_hides_older() {
        let mt = MemTable::new();
        mt.put(b"a", 5, b"v1");
        mt.delete(b"a", 10);
        assert_eq!(mt.get(b"a", 10), Some(None));
        assert_eq!(mt.get(b"a", 7), Some(Some(Bytes::from_static(b"v1"))));
    }
}
