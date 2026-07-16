//! Cursor for range scans over the LSM engine.

use std::sync::Arc;

use bytes::Bytes;

use crate::Result;
use crate::SequenceNumber;
use crate::blob::{BlobRef, BlobStore};
use crate::cache::BlockCaches;
use crate::column_family::ColumnFamilyHandle;
use crate::engine::LsmEngineInner;
use crate::immutable::sstable_path;
use crate::internal_key::{RangeTombstone, ValueType, extract_user_key, parse_internal_key};
use crate::memtable::MemTable;
use crate::merge_iter::{InternalIterator, MergeIterator};
use crate::sstable::reader::SSTableReader;
use crate::version::Version;

/// Cursor over a key range.
pub struct LsmCursor {
    inner: MergeIterator,
    start: Option<Vec<u8>>,
    end: Option<Vec<u8>>,
    snapshot: SequenceNumber,
    finished: bool,
    /// Range tombstones from all sources (memtables + SSTables) visible to this
    /// cursor.  Used to filter out point keys covered by a newer tombstone.
    range_tombstones: Vec<RangeTombstone>,
    blob_store: Arc<BlobStore>,
    /// Resolved value for the current entry.  Populated by `skip_filtered` so
    /// that blob references can be fetched once and reused by `next`.
    current_value: Option<Bytes>,
}

impl LsmCursor {
    pub(crate) fn new(
        inner: LsmEngineInner,
        start: Option<Vec<u8>>,
        end: Option<Vec<u8>>,
        snapshot: SequenceNumber,
    ) -> Result<Self> {
        Self::new_cf(inner, ColumnFamilyHandle::default(), start, end, snapshot)
    }

    pub(crate) fn new_cf(
        inner: LsmEngineInner,
        cf: ColumnFamilyHandle,
        start: Option<Vec<u8>>,
        end: Option<Vec<u8>>,
        snapshot: SequenceNumber,
    ) -> Result<Self> {
        let (memtable, immutable, version, caches, path) = {
            let state = inner.state.lock().unwrap();
            let cf = state
                .column_families
                .get(cf.id())
                .ok_or_else(|| crate::Error::InvalidArgument("column family not found".into()))?;
            let memtable = cf.memtable.lock().unwrap().clone();
            let immutable = cf.immutable.snapshot();
            let version = cf.version_set.current();
            let caches = cf.caches.clone();
            let path = state.path.clone();
            (memtable, immutable, version, caches, path)
        };
        Self::build_with_view(
            &memtable,
            &immutable,
            &version,
            &path,
            &caches,
            Arc::clone(&inner.blob_store),
            start,
            end,
            snapshot,
        )
    }

    pub(crate) fn new_cf_view(
        inner: LsmEngineInner,
        view: &crate::transaction::CfSnapshotView,
        start: Option<Vec<u8>>,
        end: Option<Vec<u8>>,
        snapshot: SequenceNumber,
    ) -> Result<Self> {
        Self::build_with_view(
            &view.memtable,
            &view.immutable,
            &view.version,
            &view.path,
            &view.caches,
            Arc::clone(&inner.blob_store),
            start,
            end,
            snapshot,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn build_with_view(
        memtable: &Arc<MemTable>,
        immutable: &[Arc<MemTable>],
        version: &Arc<Version>,
        path: &std::path::Path,
        caches: &BlockCaches,
        blob_store: Arc<BlobStore>,
        start: Option<Vec<u8>>,
        end: Option<Vec<u8>>,
        snapshot: SequenceNumber,
    ) -> Result<Self> {
        let (children, range_tombstones) = build_children(
            memtable,
            immutable,
            version,
            path,
            caches,
            start.as_deref(),
        )?;

        let mut merge = MergeIterator::new(children)?;
        if let Some(s) = start.as_deref() {
            merge.seek(s)?;
        } else {
            merge.seek_to_first()?;
        }

        let mut cursor = Self {
            inner: merge,
            start,
            end,
            snapshot,
            finished: false,
            range_tombstones,
            blob_store,
            current_value: None,
        };
        cursor.skip_filtered()?;
        Ok(cursor)
    }

    /// Advance the merge iterator, skipping entries that are outside the
    /// requested range, newer than the snapshot, or deletion tombstones.
    fn skip_filtered(&mut self) -> Result<()> {
        while self.inner.valid() && !self.finished {
            let (seq, ty) = parse_internal_key(self.inner.key()).unwrap();
            let user_key = extract_user_key(self.inner.key()).to_vec();

            if seq > self.snapshot {
                self.inner.next()?;
                continue;
            }

            if let Some(ref e) = self.end
                && user_key.as_slice() >= e.as_slice()
            {
                // Beyond the upper bound: the iterator is exhausted because
                // children are sorted.
                self.finished = true;
                break;
            }

            if let Some(ref s) = self.start
                && user_key.as_slice() < s.as_slice()
            {
                self.inner.next()?;
                continue;
            }

            if ty == ValueType::Deletion {
                // A deletion suppresses this and all older versions of the key.
                self.skip_user_key(&user_key)?;
                continue;
            }

            if self.is_covered_by_range_tombstone(&user_key, seq) {
                self.skip_user_key(&user_key)?;
                continue;
            }

            // Valid entry: resolve inline values and blob references.
            self.current_value = Some(resolve_cursor_value(
                ty,
                self.inner.value(),
                &self.blob_store,
            )?);
            break;
        }
        Ok(())
    }

    /// True if `user_key` at sequence `seq` is covered by a range tombstone
    /// that is visible to the cursor snapshot and newer than `seq`.
    fn is_covered_by_range_tombstone(&self, user_key: &[u8], seq: SequenceNumber) -> bool {
        self.range_tombstones
            .iter()
            .any(|rt| rt.seq <= self.snapshot && rt.seq >= seq && rt.covers(user_key))
    }

    /// Skip all entries that share `user_key`, including the current one.
    fn skip_user_key(&mut self, user_key: &[u8]) -> Result<()> {
        let target = user_key.to_vec();
        self.inner.next()?;
        while self.inner.valid() {
            let current_user = extract_user_key(self.inner.key());
            if current_user != target {
                break;
            }
            self.inner.next()?;
        }
        Ok(())
    }
}

impl Iterator for LsmCursor {
    type Item = Result<(Bytes, Bytes)>;

    fn next(&mut self) -> Option<Self::Item> {
        if !self.inner.valid() || self.finished {
            return None;
        }
        let key = Bytes::copy_from_slice(extract_user_key(self.inner.key()));
        let value = self
            .current_value
            .take()
            .unwrap_or_else(|| Bytes::copy_from_slice(self.inner.value()));

        // Advance past the returned entry and any older versions of the same
        // user key before looking for the next distinct key.
        if let Err(e) = self.skip_user_key(&key) {
            return Some(Err(e));
        }
        if let Err(e) = self.skip_filtered() {
            return Some(Err(e));
        }

        Some(Ok((key, value)))
    }
}

impl storage_traits::Cursor for LsmCursor {
    type Error = crate::Error;

    fn seek(&mut self, key: &[u8]) -> Result<()> {
        self.finished = false;
        self.inner.seek(key)?;
        self.skip_filtered()
    }

    fn next_batch(&mut self, limit: usize) -> Result<Vec<(Bytes, Bytes)>> {
        let mut out = Vec::with_capacity(limit);
        while out.len() < limit {
            match self.next() {
                Some(Ok(item)) => out.push(item),
                Some(Err(e)) => return Err(e),
                None => break,
            }
        }
        Ok(out)
    }
}

#[allow(clippy::type_complexity)]
fn build_children(
    memtable: &MemTable,
    immutable: &[Arc<MemTable>],
    version: &Version,
    path: &std::path::Path,
    caches: &BlockCaches,
    start: Option<&[u8]>,
) -> Result<(Vec<Box<dyn InternalIterator>>, Vec<RangeTombstone>)> {
    let mut children: Vec<Box<dyn InternalIterator>> = Vec::new();
    let mut range_tombstones: Vec<RangeTombstone> = Vec::new();

    // Mutable MemTable.
    children.push(Box::new(memtable.internal_iter()));
    range_tombstones.extend(memtable.range_tombstones());

    // Immutable MemTables, newest first.
    for mem in immutable.iter().rev() {
        children.push(Box::new(mem.internal_iter()));
        range_tombstones.extend(mem.range_tombstones());
    }

    // SSTables.
    for level in 0..version.levels.len() {
        for file in &version.levels[level] {
            // If we have bounds, skip files that cannot contain keys in range.
            if let Some(s) = start
                && file.largest.as_slice() < s
            {
                continue;
            }
            let file_path = sstable_path(path, file.number);
            let reader = SSTableReader::open(file_path, file.number, Some(caches.clone()))?;
            range_tombstones.extend(reader.range_tombstones().iter().cloned());
            let iter = reader.iter()?;
            children.push(Box::new(iter));
        }
    }

    Ok((children, range_tombstones))
}

/// Resolve a raw cursor value: inline values are copied, deletion tombstones
/// cannot reach here, and blob references are decoded and fetched.
fn resolve_cursor_value(
    ty: ValueType,
    value: &[u8],
    blob_store: &BlobStore,
) -> Result<Bytes> {
    match ty {
        ValueType::Value => Ok(Bytes::copy_from_slice(value)),
        ValueType::BlobRef => {
            let blob_ref = BlobRef::decode(value)
                .ok_or_else(|| crate::Error::Blob("bad blob reference in cursor".into()))?;
            blob_store.get(blob_ref)
        }
        ValueType::Deletion | ValueType::RangeDeletion => {
            // The cursor filtering logic should never hand a tombstone to this
            // helper; returning empty is a safe fallback.
            Ok(Bytes::new())
        }
    }
}
