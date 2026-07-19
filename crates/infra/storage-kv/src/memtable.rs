//! In-memory write buffer backed by `storage-skipmap`.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use bytes::Bytes;

use crate::SequenceNumber;
use crate::internal_key::{
    RangeTombstone, ValueType, build_internal_key, compare_internal_keys, extract_user_key,
    parse_internal_key,
};

/// A mutable in-memory table of recent writes.
pub struct MemTable {
    map: storage_skipmap::SkipMap<Vec<u8>, Bytes>,
    /// Range tombstones keyed by their start user key.  The value is the
    /// `RangeTombstone::encode` form so that a single key can hold many
    /// tombstones with the same start (they are decoded and merged on read).
    range_tombstones: storage_skipmap::SkipMap<Vec<u8>, Bytes>,
    approximate_size: AtomicUsize,
    /// Number of in-progress writes that cloned this MemTable from the engine
    /// and have not yet finished inserting.  Used to seal a MemTable before it
    /// is pushed to the immutable queue.
    pending_writes: AtomicUsize,
}

impl MemTable {
    /// Create a new empty MemTable.
    pub fn new() -> Self {
        Self {
            map: storage_skipmap::SkipMap::new(),
            range_tombstones: storage_skipmap::SkipMap::new(),
            approximate_size: AtomicUsize::new(0),
            pending_writes: AtomicUsize::new(0),
        }
    }

    /// Insert a key/value pair with the given sequence number.
    pub fn put(&self, key: &[u8], seq: SequenceNumber, value: &[u8]) {
        self.put_typed(key, seq, ValueType::Value, value);
    }

    /// Insert a key/blob-reference pair with the given sequence number.
    pub fn put_blob_ref(&self, key: &[u8], seq: SequenceNumber, blob_ref: &[u8]) {
        self.put_typed(key, seq, ValueType::BlobRef, blob_ref);
    }

    fn put_typed(&self, key: &[u8], seq: SequenceNumber, ty: ValueType, value: &[u8]) {
        let ikey = build_internal_key(key, seq, ty);
        self.approximate_size
            .fetch_add(ikey.len() + value.len(), Ordering::Relaxed);
        self.map.insert(ikey, Bytes::copy_from_slice(value));
    }

    /// Insert a deletion tombstone with the given sequence number.
    pub fn delete(&self, key: &[u8], seq: SequenceNumber) {
        let ikey = build_internal_key(key, seq, ValueType::Deletion);
        self.approximate_size
            .fetch_add(ikey.len(), Ordering::Relaxed);
        self.map.insert(ikey, Bytes::new());
    }

    /// Insert a range-deletion tombstone covering `[start, end)` as of `seq`.
    pub fn delete_range(&self, start: &[u8], end: &[u8], seq: SequenceNumber) {
        let tombstone = RangeTombstone {
            start: start.to_vec(),
            end: end.to_vec(),
            seq,
        };
        let encoded = tombstone.encode();
        self.approximate_size
            .fetch_add(encoded.len(), Ordering::Relaxed);
        self.range_tombstones
            .insert(start.to_vec(), Bytes::copy_from_slice(&encoded));
    }

    /// Look up the newest visible value for `key` at or before `snapshot_seq`.
    ///
    /// For backward compatibility this returns the raw value bytes even when the
    /// entry is a blob reference; callers that need to distinguish the type
    /// should use [`Self::get_with_type`].
    pub fn get(&self, key: &[u8], snapshot_seq: SequenceNumber) -> Option<Option<Bytes>> {
        self.get_with_type(key, snapshot_seq).map(|(_, val)| val)
    }

    /// Look up the newest visible entry for `key` at or before `snapshot_seq`,
    /// returning both the value type and the value bytes.
    pub fn get_with_type(
        &self,
        key: &[u8],
        snapshot_seq: SequenceNumber,
    ) -> Option<(ValueType, Option<Bytes>)> {
        let point = self.newest_point_entry(key, snapshot_seq);
        let tombstone_seq = self.newest_covering_tombstone(key, snapshot_seq);

        match (point, tombstone_seq) {
            // A covering range tombstone that is newer than or equal to the
            // newest point entry deletes the key for this snapshot.
            (Some((point_seq, _, _)), Some(t_seq)) if t_seq >= point_seq => {
                Some((ValueType::Deletion, None))
            }
            // Otherwise the point entry decides the result.
            (Some((_, ty, point_val)), _) => Some((ty, point_val)),
            // No point entry, but a covering tombstone means the key is deleted.
            (None, Some(_)) => Some((ValueType::Deletion, None)),
            // Truly absent.
            (None, None) => None,
        }
    }

    /// Return the newest point entry for `key` visible to `snapshot_seq`.
    ///
    /// The returned tuple is `(sequence, value_type, value_bytes)`.  `value_bytes`
    /// is `None` for deletion tombstones.
    fn newest_point_entry(
        &self,
        key: &[u8],
        snapshot_seq: SequenceNumber,
    ) -> Option<(SequenceNumber, ValueType, Option<Bytes>)> {
        // Query the skip-map for the largest internal key that is <= the
        // internal key formed from `key` and `snapshot_seq`.  Use the highest
        // point-entry type so that blob references (which sort after inline
        // values) are included in the search.
        let target = build_internal_key(key, snapshot_seq, ValueType::BlobRef);
        let (ikey, value) = self.map.floor(&target)?;
        if extract_user_key(&ikey) != key {
            return None;
        }
        let (seq, ty) = parse_internal_key(&ikey)?;
        if seq > snapshot_seq {
            return None;
        }
        let val = match ty {
            ValueType::Value | ValueType::BlobRef => Some(value),
            ValueType::Deletion => None,
            ValueType::RangeDeletion => {
                // Range tombstones are stored in the separate map, not here.
                return None;
            }
        };
        Some((seq, ty, val))
    }

    /// Return the sequence number of the newest range tombstone that covers
    /// `key` and is visible to `snapshot_seq`, or `None` if there is none.
    fn newest_covering_tombstone(
        &self,
        key: &[u8],
        snapshot_seq: SequenceNumber,
    ) -> Option<SequenceNumber> {
        let mut best: Option<SequenceNumber> = None;
        for (_, encoded) in self.range_tombstones.iter() {
            if let Some(tombstone) = RangeTombstone::decode(&encoded) {
                if tombstone.seq > snapshot_seq {
                    continue;
                }
                if tombstone.covers(key) && best.is_none_or(|b| tombstone.seq > b) {
                    best = Some(tombstone.seq);
                }
            }
        }
        best
    }

    /// Approximate byte size of the MemTable.
    pub fn approximate_size(&self) -> usize {
        self.approximate_size.load(Ordering::Relaxed)
    }

    /// Increment the pending-write counter and return a guard that decrements
    /// it when dropped.
    ///
    /// Callers must hold the engine's current-memtable lock while obtaining a
    /// guard so that a concurrent freeze cannot observe the counter as zero
    /// while a writer is about to insert.
    pub fn write_guard(self: &Arc<Self>) -> MemTableWriteGuard {
        self.pending_writes.fetch_add(1, Ordering::Acquire);
        MemTableWriteGuard {
            memtable: Arc::clone(self),
        }
    }

    /// True when no writers currently hold this MemTable.
    pub fn is_quiesced(&self) -> bool {
        self.pending_writes.load(Ordering::Acquire) == 0
    }

    /// Return all point entries in ascending internal-key order.
    ///
    /// The skip-map cursor yields entries in raw byte order, which places older
    /// sequence numbers before newer ones for the same user key.  The SSTable
    /// format and flush logic expect entries newest-first, so we re-sort using
    /// the internal-key comparator before returning.
    pub fn iter(&self) -> Vec<(Vec<u8>, Bytes)> {
        let mut entries: Vec<_> = self.map.iter().collect();
        entries.sort_by(|a, b| compare_internal_keys(&a.0, &b.0));
        entries
    }

    /// Return all range tombstones stored in this MemTable.
    pub fn range_tombstones(&self) -> Vec<RangeTombstone> {
        let mut out = Vec::new();
        for (_, encoded) in self.range_tombstones.iter() {
            if let Some(t) = RangeTombstone::decode(&encoded) {
                out.push(t);
            }
        }
        // Sort by start key so that callers (flush, compaction) see a stable
        // order and can merge them easily.
        out.sort_by(|a, b| a.start.cmp(&b.start));
        out
    }

    /// Return an internal iterator over the MemTable point entries.
    pub fn internal_iter(&self) -> MemTableIterator {
        MemTableIterator::new(self)
    }
}

/// RAII guard that decrements the MemTable's pending-write counter on drop.
///
/// Obtained via [`MemTable::write_guard`].  All inserts must go through the
/// guard so that freeze can safely seal the MemTable.
pub struct MemTableWriteGuard {
    memtable: Arc<MemTable>,
}

impl MemTableWriteGuard {
    /// Insert a key/value pair with the given sequence number.
    pub fn put(&self, key: &[u8], seq: SequenceNumber, value: &[u8]) {
        self.memtable.put(key, seq, value);
    }

    /// Insert a key/blob-reference pair with the given sequence number.
    pub fn put_blob_ref(&self, key: &[u8], seq: SequenceNumber, blob_ref: &[u8]) {
        self.memtable.put_blob_ref(key, seq, blob_ref);
    }

    /// Insert a deletion tombstone with the given sequence number.
    pub fn delete(&self, key: &[u8], seq: SequenceNumber) {
        self.memtable.delete(key, seq);
    }

    /// Insert a range-deletion tombstone covering `[start, end)` as of `seq`.
    pub fn delete_range(&self, start: &[u8], end: &[u8], seq: SequenceNumber) {
        self.memtable.delete_range(start, end, seq);
    }
}

impl Drop for MemTableWriteGuard {
    fn drop(&mut self) {
        self.memtable.pending_writes.fetch_sub(1, Ordering::Release);
    }
}

/// Iterator over a snapshot of a MemTable.
pub struct MemTableIterator {
    entries: Vec<(Vec<u8>, Bytes)>,
    position: usize,
}

impl MemTableIterator {
    fn new(memtable: &MemTable) -> Self {
        Self {
            entries: memtable.iter(),
            position: 0,
        }
    }
}

impl crate::merge_iter::InternalIterator for MemTableIterator {
    fn seek_to_first(&mut self) -> crate::Result<()> {
        self.position = 0;
        Ok(())
    }

    fn seek(&mut self, target: &[u8]) -> crate::Result<()> {
        let target_user = extract_user_key(target);
        self.position = self
            .entries
            .partition_point(|(k, _)| extract_user_key(k) < target_user);
        Ok(())
    }

    fn next(&mut self) -> crate::Result<()> {
        if self.position < self.entries.len() {
            self.position += 1;
        }
        Ok(())
    }

    fn valid(&self) -> bool {
        self.position < self.entries.len()
    }

    fn key(&self) -> &[u8] {
        &self.entries[self.position].0
    }

    fn value(&self) -> &[u8] {
        &self.entries[self.position].1
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
    fn get_ignores_newer_entries_beyond_snapshot() {
        let mt = MemTable::new();
        mt.put(b"a", 5, b"old");
        mt.put(b"a", 10, b"mid");
        mt.put(b"a", 20, b"new");
        assert_eq!(mt.get(b"a", 15), Some(Some(Bytes::from_static(b"mid"))));
        assert_eq!(mt.get(b"a", 10), Some(Some(Bytes::from_static(b"mid"))));
        assert_eq!(mt.get(b"a", 7), Some(Some(Bytes::from_static(b"old"))));
    }

    #[test]
    fn delete_hides_older() {
        let mt = MemTable::new();
        mt.put(b"a", 5, b"v1");
        mt.delete(b"a", 10);
        assert_eq!(mt.get(b"a", 10), Some(None));
        assert_eq!(mt.get(b"a", 7), Some(Some(Bytes::from_static(b"v1"))));
    }

    #[test]
    fn range_tombstone_covers_key() {
        let mt = MemTable::new();
        mt.put(b"b", 1, b"v1");
        mt.delete_range(b"a", b"d", 2);
        assert_eq!(mt.get(b"b", 2), Some(None));
        assert_eq!(mt.get(b"a", 2), Some(None));
        assert_eq!(mt.get(b"d", 2), None);
    }

    #[test]
    fn range_tombstone_respects_snapshot() {
        let mt = MemTable::new();
        mt.put(b"b", 1, b"v1");
        mt.delete_range(b"a", b"d", 5);
        // Snapshot at seq 3 predates the range tombstone.
        assert_eq!(mt.get(b"b", 3), Some(Some(Bytes::from_static(b"v1"))));
        // Snapshot at seq 5 sees the tombstone.
        assert_eq!(mt.get(b"b", 5), Some(None));
    }

    #[test]
    fn newer_put_outside_range_tombstone_wins() {
        let mt = MemTable::new();
        mt.put(b"b", 1, b"v1");
        mt.delete_range(b"a", b"d", 5);
        mt.put(b"b", 10, b"v2");
        assert_eq!(mt.get(b"b", 10), Some(Some(Bytes::from_static(b"v2"))));
    }

    #[test]
    fn older_put_inside_range_tombstone_is_deleted() {
        let mt = MemTable::new();
        mt.put(b"b", 10, b"v1");
        mt.delete_range(b"a", b"d", 5);
        // The put at seq 10 is newer than the tombstone at seq 5, so it wins.
        assert_eq!(mt.get(b"b", 10), Some(Some(Bytes::from_static(b"v1"))));
        // At seq 5 the tombstone is the newest covering entry.
        assert_eq!(mt.get(b"b", 5), Some(None));
    }
}
