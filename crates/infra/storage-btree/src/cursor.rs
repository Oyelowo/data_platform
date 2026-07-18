//! Ordered cursor over the v2 in-place B+ tree.
//!
//! A cursor pins the root page id that was current when it was created.  This
//! prevents `BPlusTree::compact` from reclaiming pages reachable from that
//! snapshot, so the cursor can safely walk leaf sibling chains even while
//! concurrent structure modifications occur.  Individual leaf pages are read
//! with optimistic lock coupling and the read is retried if a page changes
//! mid-read.
//!
//! When the cursor is created inside a write transaction it is passed the
//! transaction's write set so that range scans see the same uncommitted data as
//! point reads (read-your-writes).

use std::sync::Arc;

use bytes::Bytes;

use crate::buffer::PageGuard;
use crate::error::{Error, Result};
use crate::page::{NULL_PAGE_ID, PageId};
use crate::slot::OwnedCell;
use crate::transaction::{WriteSet, WriteSetEntry};
use crate::tree::{BPlusTree, child_for_key};
use crate::txn::{Timestamp, Transaction, TxnId};

/// A merged entry ready to be emitted by the cursor.
#[derive(Debug)]
enum CursorEntry {
    /// A cell read from a leaf page.
    Leaf(OwnedCell),
    /// A put made by the current transaction but not yet committed.
    Put(Vec<u8>, Vec<u8>),
}

/// Cursor over a key range in the B+ tree.
pub struct BPlusTreeCursor {
    tree: Arc<BPlusTree>,
    /// Root pinned for the lifetime of the cursor.
    root: PageId,
    read_ts: Timestamp,
    self_txn_id: TxnId,
    end: Option<Bytes>,
    /// Guard pinning the current leaf frame.
    current: Option<PageGuard>,
    /// Merged entries read from `current` the last time it was loaded.
    entries: Vec<CursorEntry>,
    /// Position within `entries`.
    pos: usize,
    done: bool,
    /// Last key that was actually emitted. Used to avoid duplicates after a
    /// conflict retry.
    last_returned_key: Option<Vec<u8>>,
    /// Optional transaction write set for read-your-writes.
    write_set: Option<WriteSet>,
}

impl BPlusTreeCursor {
    /// Create a cursor over `[start, end)` in `txn`'s snapshot.
    ///
    /// `write_set` is the uncommitted writes made by the same transaction.  When
    /// present, the cursor merges those writes into the scan results.
    pub fn new(
        tree: Arc<BPlusTree>,
        txn: &Transaction,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
        write_set: Option<WriteSet>,
    ) -> Result<Self> {
        let root = tree.root_page_id();
        tree.pin_root(root);
        let mut cursor = Self {
            tree,
            root,
            read_ts: txn.read_ts,
            self_txn_id: txn.txn_id,
            end: end.map(Bytes::copy_from_slice),
            current: None,
            entries: Vec::new(),
            pos: 0,
            done: false,
            last_returned_key: None,
            write_set,
        };
        match start {
            Some(key) => cursor.seek(key)?,
            None => cursor.seek_to_leftmost()?,
        }
        Ok(cursor)
    }

    /// Reposition the cursor on the first key >= `target`.
    pub fn seek(&mut self, target: &[u8]) -> Result<()> {
        loop {
            match self.try_seek(target) {
                Ok(()) => return Ok(()),
                Err(Error::Conflict) => continue,
                Err(e) => return Err(e),
            }
        }
    }

    fn seek_to_leftmost(&mut self) -> Result<()> {
        loop {
            match self.descend(None) {
                Ok(Some((guard, entries, pos))) => {
                    self.current = Some(guard);
                    self.entries = entries;
                    self.pos = pos;
                    self.done = false;
                    return Ok(());
                }
                Ok(None) => continue,
                Err(e) => return Err(e),
            }
        }
    }

    fn try_seek(&mut self, target: &[u8]) -> Result<()> {
        match self.descend(Some(target)) {
            Ok(Some((guard, entries, pos))) => {
                self.current = Some(guard);
                self.entries = entries;
                self.pos = pos;
                self.done = false;
                Ok(())
            }
            Ok(None) => Err(Error::Conflict),
            Err(e) => Err(e),
        }
    }

    /// Descend from the pinned root to the leaf that should contain `key`.
    /// Returns `None` when a page version changes during the descent.
    fn descend(&self, key: Option<&[u8]>) -> Result<Option<(PageGuard, Vec<CursorEntry>, usize)>> {
        let mut current_id = self.root;
        loop {
            let guard = self.tree.pool().fetch_or_read(current_id)?;
            let page = guard.page();
            let version = match page.optimistic_version() {
                Some(v) => v,
                None => return Ok(None),
            };

            if page.is_leaf() {
                let entries = read_leaf_entries(page, key, self.end.as_deref(), &self.write_set)?;
                let pos = match key {
                    None => 0,
                    Some(k) => entries.partition_point(|e| cursor_entry_key(e).as_slice() < k),
                };
                if page.latch_word() != version {
                    return Ok(None);
                }
                return Ok(Some((guard, entries, pos)));
            }

            let child_id = match key {
                None => page.leftmost_child()?,
                Some(k) => child_for_key(page, k)?,
            };
            if page.latch_word() != version {
                return Ok(None);
            }
            // A poisoned old root may present a null child pointer; retry rather
            // than fetch a null page id.
            if child_id == NULL_PAGE_ID {
                return Ok(None);
            }
            current_id = child_id;
        }
    }

    /// Move to the next leaf in the sibling chain.  Returns `false` when there
    /// is no next leaf or a concurrent modification forces a retry.
    fn advance_leaf(&mut self) -> Result<bool> {
        let guard = match self.current.take() {
            Some(g) => g,
            None => {
                self.done = true;
                return Ok(false);
            }
        };
        let page = guard.page();
        let version = match page.optimistic_version() {
            Some(v) => v,
            None => {
                // Page changed; retry from the last key we saw.
                self.current = Some(guard);
                return self.retry_from_last_key();
            }
        };
        let next_id = page.next_page_id()?;
        if page.latch_word() != version {
            self.current = Some(guard);
            return self.retry_from_last_key();
        }
        drop(guard);

        if next_id == NULL_PAGE_ID {
            self.done = true;
            return Ok(false);
        }

        let next_guard = self.tree.pool().fetch_or_read(next_id)?;
        let next_page = next_guard.page();
        let next_version = match next_page.optimistic_version() {
            Some(v) => v,
            None => {
                // Next page is locked; retry from the last key.
                drop(next_guard);
                return self.retry_from_last_key();
            }
        };
        let entries = read_leaf_entries(next_page, None, self.end.as_deref(), &self.write_set)?;
        if next_page.latch_word() != next_version {
            drop(next_guard);
            return self.retry_from_last_key();
        }

        self.current = Some(next_guard);
        self.entries = entries;
        self.pos = 0;
        Ok(true)
    }

    /// Re-traverse from the pinned root to just after the last emitted key.
    fn retry_from_last_key(&mut self) -> Result<bool> {
        match self.last_returned_key.as_ref() {
            None => {
                // No state to resume from; start over at the leftmost leaf.
                self.seek_to_leftmost()?;
            }
            Some(last_key) => {
                // Seek to the leaf containing the last emitted key, then skip
                // over any entries equal to it so we never emit the same key
                // twice.
                let last_key = last_key.clone();
                self.seek(&last_key)?;
                while self.pos < self.entries.len()
                    && cursor_entry_key(&self.entries[self.pos]).as_slice() == last_key
                {
                    self.pos += 1;
                }
            }
        }
        Ok(true)
    }

    fn resolve_value(&self, cell: &OwnedCell) -> Result<Option<Vec<u8>>> {
        self.tree
            .resolve_cell_value(cell, self.read_ts, self.self_txn_id)
    }
}

impl Iterator for BPlusTreeCursor {
    type Item = Result<(Bytes, Bytes)>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        loop {
            if self.pos >= self.entries.len() {
                match self.advance_leaf() {
                    Ok(true) => continue,
                    Ok(false) => return None,
                    Err(e) => return Some(Err(e)),
                }
            }

            let entry = &self.entries[self.pos];
            self.pos += 1;

            let (key, value) = match entry {
                CursorEntry::Leaf(cell) => {
                    if let Some(ref end) = self.end
                        && cell.key.as_slice() >= end.as_ref()
                    {
                        self.done = true;
                        return None;
                    }
                    match self.resolve_value(cell) {
                        Ok(Some(value)) => (cell.key.clone(), value),
                        Ok(None) => continue,
                        Err(e) => return Some(Err(e)),
                    }
                }
                CursorEntry::Put(key, value) => {
                    if let Some(ref end) = self.end
                        && key.as_slice() >= end.as_ref()
                    {
                        self.done = true;
                        return None;
                    }
                    (key.clone(), value.clone())
                }
            };

            self.last_returned_key = Some(key.clone());
            return Some(Ok((
                Bytes::copy_from_slice(&key),
                Bytes::copy_from_slice(&value),
            )));
        }
    }
}

impl storage_traits::Cursor for BPlusTreeCursor {
    type Error = Error;

    fn seek(&mut self, target: &[u8]) -> Result<()> {
        Self::seek(self, target)
    }
}

impl Drop for BPlusTreeCursor {
    fn drop(&mut self) {
        self.tree.unpin_root(self.root);
    }
}

fn cursor_entry_key(entry: &CursorEntry) -> Vec<u8> {
    match entry {
        CursorEntry::Leaf(cell) => cell.key.clone(),
        CursorEntry::Put(key, _) => key.clone(),
    }
}

/// Read all live cells from a leaf page and merge in any uncommitted writes
/// from `write_set` that fall within the leaf's key range.
///
/// `leaf_start` is an optional lower bound for the leaf (the key the cursor
/// used to descend, or `None` when advancing through the sibling chain).  It
/// is used to filter write-set entries that belong to earlier leaves.
fn read_leaf_entries(
    page: &crate::page::Page,
    leaf_start: Option<&[u8]>,
    end: Option<&[u8]>,
    write_set: &Option<WriteSet>,
) -> Result<Vec<CursorEntry>> {
    let leaf_first: Option<Vec<u8>> = leaf_start
        .map(|k| k.to_vec())
        .or_else(|| page.first_key().ok().flatten());
    let leaf_last = page.last_key().ok().flatten();

    let mut entries: Vec<CursorEntry> = Vec::with_capacity(page.slot_count()?);
    let count = page.slot_count()?;
    for idx in 0..count {
        if page.read_slot(idx)?.is_deleted() {
            continue;
        }
        let cell = page.get_by_slot(idx)?;
        // If the cell is before the leaf start, skip it.  This can happen when
        // seeking to a key and the leaf contains earlier keys.
        if let Some(start) = leaf_start
            && cell.key.as_slice() < start
        {
            continue;
        }
        // If the cell is at or past the scan end, stop reading this leaf.
        if let Some(e) = end
            && cell.key.as_slice() >= e
        {
            break;
        }
        entries.push(CursorEntry::Leaf(cell));
    }

    if let Some(ws) = write_set {
        let ws = ws.read();
        let start_key = leaf_first.unwrap_or_default();
        let ws_entries: Vec<(Vec<u8>, WriteSetEntry)> = ws
            .range(start_key..)
            .take_while(|(k, _)| {
                if let Some(e) = end
                    && k.as_slice() >= e
                {
                    return false;
                }
                if let Some(ref l) = leaf_last
                    && k.as_slice() > l.as_slice()
                {
                    return false;
                }
                true
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        for (key, entry) in ws_entries {
            match entry {
                WriteSetEntry::Put(value) => {
                    // Overwrite an existing leaf cell or insert a new one.
                    if let Some(pos) = entries
                        .iter()
                        .position(|e| cursor_entry_key(e).as_slice() == key.as_slice())
                    {
                        entries[pos] = CursorEntry::Put(key, value);
                    } else {
                        entries.push(CursorEntry::Put(key, value));
                    }
                }
                WriteSetEntry::Delete => {
                    // Hide the leaf cell if present.
                    entries.retain(|e| cursor_entry_key(e).as_slice() != key.as_slice());
                }
            }
        }
    }

    entries.sort_by_key(cursor_entry_key);
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::BufferPool;
    use crate::disk::PagedFile;
    use crate::space::PageAllocator;
    use crate::sync::Mutex as SyncMutex;
    use crate::txn::IsolationLevel;

    fn make_tree() -> (Arc<BPlusTree>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let disk = Arc::new(PagedFile::open(dir.path().join("pages.dat"), 512).unwrap());
        let alloc = Arc::new(SyncMutex::new(PageAllocator::new(PageId::new(1))));
        let pool = Arc::new(BufferPool::new(64, 512, disk, alloc).unwrap());
        (Arc::new(BPlusTree::new(pool, 64).unwrap()), dir)
    }

    #[test]
    fn cursor_scans_all_keys() {
        let (tree, _dir) = make_tree();
        for i in 0u64..10 {
            let key = format!("{:02x}", i);
            tree.insert(key.as_bytes(), key.as_bytes()).unwrap();
        }

        let txn = tree.begin_txn(IsolationLevel::Snapshot).unwrap();
        let cursor = BPlusTreeCursor::new(tree.clone(), &txn, None, None, None).unwrap();
        let keys: Vec<String> = cursor
            .map(|r| {
                let (k, _v) = r.unwrap();
                String::from_utf8(k.to_vec()).unwrap()
            })
            .collect();
        assert_eq!(keys.len(), 10);
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
    }

    #[test]
    fn cursor_respects_range_bounds() {
        let (tree, _dir) = make_tree();
        for i in 0u64..10 {
            let key = format!("{:02x}", i);
            tree.insert(key.as_bytes(), b"v").unwrap();
        }

        let txn = tree.begin_txn(IsolationLevel::Snapshot).unwrap();
        let cursor =
            BPlusTreeCursor::new(tree.clone(), &txn, Some(b"03"), Some(b"07"), None).unwrap();
        let keys: Vec<String> = cursor
            .map(|r| String::from_utf8(r.unwrap().0.to_vec()).unwrap())
            .collect();
        assert_eq!(keys, vec!["03", "04", "05", "06"]);
    }

    #[test]
    fn cursor_seek_repositions() {
        let (tree, _dir) = make_tree();
        for i in 0u64..10 {
            let key = format!("{:02x}", i);
            tree.insert(key.as_bytes(), b"v").unwrap();
        }

        let txn = tree.begin_txn(IsolationLevel::Snapshot).unwrap();
        let mut cursor = BPlusTreeCursor::new(tree.clone(), &txn, None, None, None).unwrap();
        cursor.seek(b"05").unwrap();
        let keys: Vec<String> = cursor
            .map(|r| String::from_utf8(r.unwrap().0.to_vec()).unwrap())
            .collect();
        assert_eq!(keys, vec!["05", "06", "07", "08", "09"]);
    }

    #[test]
    fn cursor_sees_stable_snapshot() {
        let (tree, _dir) = make_tree();
        for i in 0u64..5 {
            let key = format!("{:02x}", i);
            tree.insert(key.as_bytes(), b"v").unwrap();
        }

        let txn = tree.begin_txn(IsolationLevel::Snapshot).unwrap();
        let cursor = BPlusTreeCursor::new(tree.clone(), &txn, None, None, None).unwrap();

        // Concurrent commit after cursor creation.
        let t2 = tree.begin_txn(IsolationLevel::Snapshot).unwrap();
        tree.insert_txn(&t2, b"ff", b"new").unwrap();
        tree.commit_txn(&t2).unwrap();

        let keys: Vec<String> = cursor
            .map(|r| String::from_utf8(r.unwrap().0.to_vec()).unwrap())
            .collect();
        assert!(!keys.contains(&"ff".to_string()));
        assert_eq!(keys.len(), 5);
    }

    #[test]
    fn cursor_skips_deleted_keys() {
        let (tree, _dir) = make_tree();
        for i in 0u64..5 {
            let key = format!("{:02x}", i);
            tree.insert(key.as_bytes(), b"v").unwrap();
        }
        tree.delete(b"02").unwrap();
        tree.delete(b"04").unwrap();

        let txn = tree.begin_txn(IsolationLevel::Snapshot).unwrap();
        let cursor = BPlusTreeCursor::new(tree.clone(), &txn, None, None, None).unwrap();
        let keys: Vec<String> = cursor
            .map(|r| String::from_utf8(r.unwrap().0.to_vec()).unwrap())
            .collect();
        assert_eq!(keys, vec!["00", "01", "03"]);
    }
}
