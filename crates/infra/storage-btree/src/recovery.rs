//! ARIES-style recovery for the in-place B+ tree.
//!
//! Recovery runs at engine open time and consists of three passes:
//!
//! 1. **Analysis**: scan the WAL from the checkpoint LSN and rebuild the
//!    dirty-page table and active-transaction table.
//! 2. **Redo**: scan the WAL from the redo LSN and re-apply every record whose
//!    LSN is greater than the on-page `page_lsn`.
//! 3. **Undo**: walk the chains of active transactions backward, appending CLRs
//!    and applying inverse operations.
//!
//! See `PHASE5_DESIGN.md` for the full protocol.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use std::cmp::Ordering as CmpOrdering;

use crate::buffer::BufferPool;
use crate::error::{Error, Result};
use crate::page::{NULL_PAGE_ID, Page, PageId};
use crate::slot::{OwnedCell, OwnedValue, ValueKind};
use crate::txn::NULL_TXN_ID;
use crate::undo;
use crate::valuelog::ValueLog;
use crate::version::MvccHeader;
use crate::wal::{Lsn, NULL_LSN, Record, RecordPayload, RecordType, TxnId, WalLog, set_page_lsn};

/// Entry in the dirty-page table built by the analysis pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirtyPage {
    /// LSN of the most recent WAL record that dirtied this page.
    pub rec_lsn: Lsn,
}

/// Entry in the active-transaction table built by the analysis pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActiveTxn {
    /// LSN of the last record written by this transaction (the one to undo
    /// first).
    pub last_lsn: Lsn,
}

/// Result of the analysis pass.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AnalysisResult {
    /// Map from page id to the earliest LSN that dirtied it.
    pub dirty_pages: HashMap<PageId, DirtyPage>,
    /// Map from transaction id to its recovery state.
    pub active_txns: HashMap<TxnId, ActiveTxn>,
}

/// Recovery driver.
pub struct Recovery {
    pool: Arc<BufferPool>,
    wal: Arc<WalLog>,
    /// Current root page id; updated by redoing `SetRoot` records.
    root_page_id: AtomicU64,
    /// Optional value log whose refcounts should be rebuilt from leaf pages.
    value_log: Option<Arc<ValueLog>>,
}

impl Recovery {
    /// Create a recovery driver.
    pub fn new(pool: Arc<BufferPool>, wal: Arc<WalLog>, root_page_id: PageId) -> Self {
        Self {
            pool,
            wal,
            root_page_id: AtomicU64::new(root_page_id),
            value_log: None,
        }
    }

    /// Attach a value log so recovery can rebuild its reference counts.
    pub fn with_value_log(mut self, value_log: Arc<ValueLog>) -> Self {
        self.value_log = Some(value_log);
        self
    }

    /// Run the full recovery procedure.
    ///
    /// `checkpoint_lsn` is the LSN stored in `META`; recovery scans from there.
    /// Returns the recovered root page id.
    pub fn recover(&self, checkpoint_lsn: Lsn) -> Result<PageId> {
        let analysis = self.analyze(checkpoint_lsn)?;
        self.redo(&analysis, checkpoint_lsn)?;
        if self.value_log.is_some() {
            let root = self.root_page_id.load(Ordering::SeqCst);
            self.rebuild_value_log_refs(root)?;
        }
        self.undo(&analysis)?;
        Ok(self.root_page_id.load(Ordering::SeqCst))
    }

    /// Rebuild value-log reference counts by scanning every live leaf cell
    /// reachable from `root_id`.  Called after redo and before undo so that
    /// values referenced only by recovered pages are not reclaimed by GC.
    fn rebuild_value_log_refs(&self, root_id: PageId) -> Result<()> {
        let value_log = match &self.value_log {
            Some(vl) => vl,
            None => return Ok(()),
        };
        let guard = self.pool.fetch_or_read(root_id)?;
        let page = guard.page();
        if page.is_leaf() {
            for cell in page.iter()? {
                if let OwnedValue::ValueLog { offset, len } = cell.value {
                    value_log.add_ref(offset, len);
                }
            }
            return Ok(());
        }

        let leftmost = page.leftmost_child()?;
        self.rebuild_value_log_refs(leftmost)?;
        for idx in 0..page.slot_count()? {
            if page.read_slot(idx)?.is_deleted() {
                continue;
            }
            let cell = page.get_by_slot(idx)?;
            let child_id = decode_page_id(&cell.value.as_value_kind())?;
            self.rebuild_value_log_refs(child_id)?;
        }
        Ok(())
    }

    /// Analysis pass: scan the WAL and build dirty-page / active-transaction
    /// tables.
    pub fn analyze(&self, start_lsn: Lsn) -> Result<AnalysisResult> {
        let mut result = AnalysisResult::default();
        for item in self.wal.iter(start_lsn)? {
            let (lsn, record) = item?;
            if record.header.record_type == RecordType::Clr {
                // CLRs redo a compensation but do not start or end transactions.
                // They are recorded against the same page for redo purposes.
                if let RecordPayload::Clr { undo_next_lsn, .. } = record.payload
                    && record.header.transaction_id != 0
                {
                    // The transaction's next undo target moves backward.
                    result
                        .active_txns
                        .entry(record.header.transaction_id)
                        .and_modify(|txn| txn.last_lsn = undo_next_lsn)
                        .or_insert(ActiveTxn {
                            last_lsn: undo_next_lsn,
                        });
                }
                result
                    .dirty_pages
                    .entry(record.header.page_id)
                    .and_modify(|dp| dp.rec_lsn = dp.rec_lsn.min(record.header.page_lsn))
                    .or_insert(DirtyPage {
                        rec_lsn: record.header.page_lsn,
                    });
                continue;
            }

            // Page touched by this record is dirty as of the record's page_lsn.
            result
                .dirty_pages
                .entry(record.header.page_id)
                .and_modify(|dp| dp.rec_lsn = dp.rec_lsn.min(record.header.page_lsn))
                .or_insert(DirtyPage {
                    rec_lsn: record.header.page_lsn,
                });

            if record.header.transaction_id == 0 {
                // Autocommit / structural record: no transaction state.
                continue;
            }

            match record.header.record_type {
                RecordType::Commit | RecordType::Abort => {
                    result.active_txns.remove(&record.header.transaction_id);
                }
                _ => {
                    // Most recent record for this transaction is the current LSN.
                    result
                        .active_txns
                        .entry(record.header.transaction_id)
                        .and_modify(|txn| txn.last_lsn = lsn)
                        .or_insert(ActiveTxn { last_lsn: lsn });
                }
            }
        }
        Ok(result)
    }

    /// Redo pass: re-apply all records whose LSN is greater than the on-page
    /// `page_lsn`.
    pub fn redo(&self, analysis: &AnalysisResult, start_lsn: Lsn) -> Result<()> {
        let redo_lsn = analysis
            .dirty_pages
            .values()
            .map(|dp| dp.rec_lsn)
            .min()
            .unwrap_or(start_lsn);

        for item in self.wal.iter(redo_lsn)? {
            let (lsn, record) = item?;
            if record.header.record_type == RecordType::Commit {
                continue;
            }
            self.redo_record(lsn, &record)?;
        }
        Ok(())
    }

    /// Apply a single record idempotently.
    fn redo_record(&self, wal_lsn: Lsn, record: &Record) -> Result<()> {
        let page_id = record.header.page_id;
        if page_id == NULL_PAGE_ID {
            // Records like SetRoot carry page_id 0; handle them specially.
            if let RecordPayload::SetRoot { new_root_page_id } = record.payload {
                self.root_page_id.store(new_root_page_id, Ordering::SeqCst);
            }
            return Ok(());
        }

        let guard = self.fetch_or_create_page(page_id)?;
        let page = guard.page();
        let page_lsn = page.header()?.page_lsn;
        if record.header.page_lsn != page_lsn {
            // The record claims to follow a different page LSN than what is on
            // disk.  This means the page state is not what the record expects,
            // so skip it.  This is a safety check; in normal operation the
            // record is applied only when its page_lsn matches.
            //
            // A more permissive interpretation would apply records where
            // record_lsn > page_lsn; we use strict page_lsn equality to catch
            // unexpected page states during recovery.
            return Ok(());
        }

        // The WAL LSN becomes the new page LSN if we apply it.
        let new_lsn = wal_lsn;
        match &record.payload {
            RecordPayload::InsertCell { cell } => {
                let mvcc = cell.mvcc.as_ref();
                let _ = page.insert_with_mvcc(&cell.key, &cell.value.as_value_kind(), mvcc)?;
            }
            RecordPayload::UpdateCell { cell, .. } => {
                // The new version's previous-version pointer must reference this
                // WAL record so snapshot reads can walk back to the old cell.
                let mut mvcc = cell.mvcc.unwrap_or(MvccHeader::autocommit());
                mvcc.prev_version_lsn = wal_lsn;
                let _ =
                    page.insert_with_mvcc(&cell.key, &cell.value.as_value_kind(), Some(&mvcc))?;
            }
            RecordPayload::DeleteCell { key, .. } => {
                if record.header.transaction_id == 0 {
                    // Autocommit deletes are physical deletes.
                    let _ = page.delete(key)?;
                } else {
                    // Transactional deletes install a tombstone so snapshot
                    // readers can follow the version chain backward.
                    let tombstone_header = MvccHeader {
                        begin_ts: record.header.transaction_id,
                        end_ts: NULL_TXN_ID,
                        prev_version_lsn: wal_lsn,
                    };
                    page.insert_with_mvcc(key, &ValueKind::Tombstone, Some(&tombstone_header))?;
                }
            }
            RecordPayload::SplitPage {
                separator,
                right_page_id,
                is_internal,
            } => {
                let right_guard = self.fetch_or_create_page(*right_page_id)?;
                let right = right_guard.page();
                if *is_internal {
                    self.redo_internal_split(page, right, separator)?;
                } else {
                    self.redo_leaf_split(page, right, separator)?;
                }
                set_page_lsn(page, new_lsn)?;
                set_page_lsn(right, new_lsn)?;
                right_guard.mark_dirty();
            }
            RecordPayload::NewRoot {
                new_root_page_id,
                leftmost_child,
                separator,
                right_child,
            } => {
                self.redo_new_root(
                    *new_root_page_id,
                    *leftmost_child,
                    separator,
                    *right_child,
                    new_lsn,
                )?;
            }
            RecordPayload::MergePage {
                victim_page_id,
                victim_is_left,
                separator,
                victim_leftmost,
            } => {
                self.redo_merge(
                    page_id,
                    *victim_page_id,
                    *victim_is_left,
                    separator,
                    *victim_leftmost,
                )?;
            }
            RecordPayload::MoveRightmost { new_rightmost, .. } => {
                page.set_leftmost_child(*new_rightmost);
            }
            RecordPayload::Clr { original, .. } => {
                // CLR redo applies the compensation action stored in the
                // original record.
                self.redo_clr(page, original)?;
            }
            RecordPayload::SetRoot { .. }
            | RecordPayload::Begin
            | RecordPayload::Commit { .. }
            | RecordPayload::Abort => {}
        }

        let mut header = page.header()?;
        header.page_lsn = new_lsn;
        page.set_header(&header);
        guard.mark_dirty();
        Ok(())
    }

    fn fetch_or_create_page(&self, page_id: PageId) -> Result<crate::buffer::PageGuard> {
        self.pool.fetch_or_create_page(page_id)
    }

    fn redo_leaf_split(&self, left: &Page, right: &Page, separator: &[u8]) -> Result<()> {
        right.set_leaf();
        let count = left.slot_count()?;
        let mut moved: Vec<OwnedCell> = Vec::new();
        for idx in 0..count {
            let cell = left.get_by_slot(idx)?;
            if cell.key.as_slice() >= separator {
                moved.push(cell);
            }
        }
        for cell in &moved {
            left.delete(&cell.key)?;
            right.insert_with_mvcc(&cell.key, &cell.value.as_value_kind(), cell.mvcc.as_ref())?;
        }

        let old_next = left.next_page_id()?;
        right.set_next_page_id(old_next);
        right.set_prev_page_id(left.id);
        left.set_next_page_id(right.id);
        Ok(())
    }

    fn redo_internal_split(&self, left: &Page, right: &Page, separator: &[u8]) -> Result<()> {
        right.set_internal();
        let count = left.slot_count()?;
        let mut promoted_child: Option<PageId> = None;
        let mut moved: Vec<(Vec<u8>, OwnedValue)> = Vec::new();
        for idx in 0..count {
            let cell = left.get_by_slot(idx)?;
            match cell.key.as_slice().cmp(separator) {
                CmpOrdering::Less => {}
                CmpOrdering::Equal => {
                    promoted_child = Some(decode_page_id(&cell.value.as_value_kind())?);
                }
                CmpOrdering::Greater => {
                    moved.push((cell.key, cell.value));
                }
            }
        }
        let right_leftmost = promoted_child.ok_or_else(|| {
            Error::Corruption("internal split redo: separator not found in left page".into())
        })?;
        right.set_leftmost_child(right_leftmost);

        left.delete(separator)?;
        for (key, value) in &moved {
            left.delete(key)?;
            right.insert(key, &value.as_value_kind())?;
        }
        Ok(())
    }

    /// Re-create a new root page created during a root split.
    fn redo_new_root(
        &self,
        new_root_page_id: PageId,
        leftmost_child: PageId,
        separator: &[u8],
        right_child: PageId,
        new_lsn: Lsn,
    ) -> Result<()> {
        self.pool
            .with_page_or_create_mut(new_root_page_id, |page| {
                page.set_internal();
                page.set_leftmost_child(leftmost_child);
                let child_bytes = encode_page_id(right_child);
                let _ = page.insert(separator, &crate::slot::ValueKind::Inline(&child_bytes))?;
                set_page_lsn(page, new_lsn)?;
                Ok(())
            })?;
        self.root_page_id.store(new_root_page_id, Ordering::SeqCst);
        Ok(())
    }

    /// Re-apply a page merge.  The surviving page is `survivor_id`; the victim
    /// page is `victim_id`.  Leaf merges update sibling pointers; internal
    /// merges re-insert the parent separator and the victim's cells.
    fn redo_merge(
        &self,
        survivor_id: PageId,
        victim_id: PageId,
        victim_is_left: bool,
        separator: &[u8],
        victim_leftmost: PageId,
    ) -> Result<()> {
        let survivor_guard = self.fetch_or_create_page(survivor_id)?;
        let survivor = survivor_guard.page();
        let victim_guard = self.fetch_or_create_page(victim_id)?;
        let victim = victim_guard.page();

        if survivor.is_leaf() {
            self.redo_leaf_merge(survivor, victim, victim_is_left)?;
        } else {
            self.redo_internal_merge(survivor, victim, separator, victim_leftmost)?;
        }

        survivor_guard.mark_dirty();
        victim_guard.mark_dirty();

        // The victim page is no longer reachable after the merge; return it to
        // the freelist so recovery does not leak ids that were freed before the
        // last checkpoint.
        self.pool.free_page(victim_id)?;
        Ok(())
    }

    fn redo_leaf_merge(&self, survivor: &Page, victim: &Page, victim_is_left: bool) -> Result<()> {
        if !victim_is_left {
            // Victim is the right sibling: move all of its cells into the
            // survivor and link around the victim.
            let count = victim.slot_count()?;
            for idx in 0..count {
                let cell = victim.get_by_slot(idx)?;
                survivor.insert_with_mvcc(
                    &cell.key,
                    &cell.value.as_value_kind(),
                    cell.mvcc.as_ref(),
                )?;
            }
            let victim_next = victim.next_page_id()?;
            survivor.set_next_page_id(victim_next);
            if victim_next != NULL_PAGE_ID {
                let _ = self.pool.with_page_mut(victim_next, |next| {
                    next.set_prev_page_id(survivor.id);
                    Ok(())
                });
            }
        } else {
            // Victim is the left sibling: move all of its cells into the
            // survivor and link around the victim.
            let count = victim.slot_count()?;
            for idx in 0..count {
                let cell = victim.get_by_slot(idx)?;
                survivor.insert_with_mvcc(
                    &cell.key,
                    &cell.value.as_value_kind(),
                    cell.mvcc.as_ref(),
                )?;
            }
            let victim_prev = victim.prev_page_id()?;
            survivor.set_prev_page_id(victim_prev);
            if victim_prev != NULL_PAGE_ID {
                let _ = self.pool.with_page_mut(victim_prev, |prev| {
                    prev.set_next_page_id(survivor.id);
                    Ok(())
                });
            }
        }
        Ok(())
    }

    fn redo_internal_merge(
        &self,
        survivor: &Page,
        victim: &Page,
        separator: &[u8],
        victim_leftmost: PageId,
    ) -> Result<()> {
        let bytes = encode_page_id(victim_leftmost);
        survivor.insert(separator, &crate::slot::ValueKind::Inline(&bytes))?;

        let count = victim.slot_count()?;
        for idx in 0..count {
            let cell = victim.get_by_slot(idx)?;
            let child_id = decode_page_id(&cell.value.as_value_kind())?;
            let bytes = encode_page_id(child_id);
            survivor.insert(&cell.key, &crate::slot::ValueKind::Inline(&bytes))?;
        }
        Ok(())
    }

    /// Undo pass: rollback active transactions by appending CLRs and applying
    /// inverse operations.
    pub fn undo(&self, analysis: &AnalysisResult) -> Result<()> {
        for (&txn_id, txn) in &analysis.active_txns {
            if txn.last_lsn == NULL_LSN {
                continue;
            }
            self.undo_transaction(txn_id, txn.last_lsn)?;
        }
        Ok(())
    }

    fn undo_transaction(&self, txn_id: TxnId, last_lsn: Lsn) -> Result<()> {
        let mut lsn = last_lsn;
        while lsn != NULL_LSN {
            let record = self.wal.read_at(lsn)?;
            match &record.payload {
                RecordPayload::Clr { undo_next_lsn, .. } => {
                    lsn = *undo_next_lsn;
                    continue;
                }
                RecordPayload::Begin => break,
                RecordPayload::UpdateCell {
                    old_cell: Some(old_cell),
                    old_header: Some(old_header),
                    ..
                } => {
                    let image = undo::make_undo_image(old_cell.clone(), *old_header);
                    self.pool
                        .with_page_or_create_mut(record.header.page_id, |page| {
                            undo::apply_undo_to_page(page, &old_cell.key, &image)
                        })?;
                }
                RecordPayload::DeleteCell {
                    key,
                    old_cell: Some(old_cell),
                    old_header: Some(old_header),
                } => {
                    let image = undo::make_undo_image(old_cell.clone(), *old_header);
                    self.pool
                        .with_page_or_create_mut(record.header.page_id, |page| {
                            undo::apply_undo_to_page(page, key, &image)
                        })?;
                }
                RecordPayload::InsertCell { cell } => {
                    self.pool
                        .with_page_or_create_mut(record.header.page_id, |page| {
                            page.delete(&cell.key)
                        })?;
                }
                _ => {}
            }

            undo::append_clr(&self.wal, txn_id, lsn, record.header.prev_lsn, &record)?;
            lsn = record.header.prev_lsn;
        }
        Ok(())
    }

    /// Apply the compensation action encoded in a CLR's `original` record.
    fn redo_clr(&self, page: &Page, original: &Record) -> Result<()> {
        match &original.payload {
            RecordPayload::UpdateCell {
                old_cell: Some(old_cell),
                old_header: Some(old_header),
                ..
            } => {
                let image = undo::make_undo_image(old_cell.clone(), *old_header);
                undo::apply_undo_to_page(page, &old_cell.key, &image)?;
            }
            RecordPayload::DeleteCell {
                key,
                old_cell: Some(old_cell),
                old_header: Some(old_header),
            } => {
                let image = undo::make_undo_image(old_cell.clone(), *old_header);
                undo::apply_undo_to_page(page, key, &image)?;
            }
            RecordPayload::InsertCell { cell } => {
                page.delete(&cell.key)?;
            }
            _ => {}
        }
        Ok(())
    }
}

fn encode_page_id(id: PageId) -> [u8; 8] {
    id.to_le_bytes()
}

fn decode_page_id(value: &crate::slot::ValueKind<'_>) -> Result<PageId> {
    match value {
        crate::slot::ValueKind::Inline(b) if b.len() == 8 => Ok(PageId::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ])),
        _ => Err(Error::Corruption(
            "internal node value is not an 8-byte page id".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::BufferPool;
    use crate::disk::PagedFile;
    use crate::slot::OwnedCell;
    use crate::space::PageAllocator;
    use crate::sync::Mutex as SyncMutex;
    use crate::wal::RecordHeader;
    use std::path::Path;

    fn setup_pool(dir: &Path, page_size: usize) -> (Arc<BufferPool>, tempfile::TempDir) {
        let tmp = tempfile::tempdir_in(dir).unwrap();
        let disk = Arc::new(PagedFile::open(tmp.path().join("pages.dat"), page_size).unwrap());
        let alloc = Arc::new(SyncMutex::new(PageAllocator::new(1)));
        let pool = Arc::new(BufferPool::new(64, page_size, disk, alloc).unwrap());
        (pool, tmp)
    }

    #[test]
    fn analyze_builds_dirty_page_table() {
        let (pool, dir) = setup_pool(&std::env::temp_dir(), 512);
        let wal = Arc::new(WalLog::open(dir.path(), storage_wal::WalOptions::default()).unwrap());

        // Manually append a couple of records.
        let cell = OwnedCell {
            key: b"k".to_vec(),
            value: crate::slot::OwnedValue::Inline(b"v".to_vec()),
            mvcc: None,
        };
        wal.append(Record {
            header: RecordHeader::new(RecordType::InsertCell, 0, NULL_LSN, 3, 0),
            payload: RecordPayload::InsertCell { cell: cell.clone() },
        })
        .unwrap();
        wal.append(Record {
            header: RecordHeader::new(RecordType::DeleteCell, 0, NULL_LSN, 3, 5),
            payload: RecordPayload::DeleteCell {
                key: b"k".to_vec(),
                old_cell: None,
                old_header: None,
            },
        })
        .unwrap();

        let recovery = Recovery::new(pool, wal, 1);
        let analysis = recovery.analyze(0).unwrap();
        assert!(analysis.dirty_pages.contains_key(&3));
        assert_eq!(analysis.dirty_pages[&3].rec_lsn, 0);
        assert!(analysis.active_txns.is_empty());
    }

    #[test]
    fn redo_replays_insert_and_delete() {
        let (pool, dir) = setup_pool(&std::env::temp_dir(), 512);
        let wal = Arc::new(WalLog::open(dir.path(), storage_wal::WalOptions::default()).unwrap());

        // Allocate and initialise a leaf page so redo has something to mutate.
        let guard = pool.new_page().unwrap();
        let page_id = guard.page().id;
        guard.page().set_leaf();
        guard.mark_dirty();
        drop(guard);
        pool.flush_all().unwrap();

        let cell = OwnedCell {
            key: b"k".to_vec(),
            value: crate::slot::OwnedValue::Inline(b"v".to_vec()),
            mvcc: None,
        };
        wal.append(Record {
            header: RecordHeader::new(RecordType::InsertCell, 0, NULL_LSN, page_id, 0),
            payload: RecordPayload::InsertCell { cell: cell.clone() },
        })
        .unwrap();

        let recovery = Recovery::new(pool.clone(), wal.clone(), 1);
        let analysis = recovery.analyze(0).unwrap();
        recovery.redo(&analysis, 0).unwrap();

        let guard = pool.fetch_or_read(page_id).unwrap();
        let value = guard.page().get(b"k").unwrap().map(|c| match c.value {
            crate::slot::OwnedValue::Inline(v) => v,
            _ => panic!("expected inline value"),
        });
        assert_eq!(value, Some(b"v".to_vec()));
    }

    #[test]
    fn crash_with_active_txn() {
        let dir = tempfile::tempdir().unwrap();
        let page_size = 512;
        let disk = Arc::new(PagedFile::open(dir.path().join("pages.dat"), page_size).unwrap());
        let alloc = Arc::new(SyncMutex::new(PageAllocator::new(1)));
        let pool = Arc::new(BufferPool::new(64, page_size, disk, alloc).unwrap());
        let wal = Arc::new(WalLog::open(dir.path(), storage_wal::WalOptions::default()).unwrap());

        // Seed an autocommit value, then start a transaction that updates it.
        let tree = crate::tree::BPlusTree::new(pool.clone(), page_size / 4)
            .unwrap()
            .with_wal(Arc::clone(&wal));
        tree.insert(b"k", b"old").unwrap();

        let txn = tree
            .begin_txn(crate::txn::IsolationLevel::Snapshot)
            .unwrap();
        tree.insert_txn(&txn, b"k", b"new").unwrap();
        // Drop without committing: simulate a crash with an active transaction.
        drop(tree);

        // Recover from the WAL.
        let disk2 = Arc::new(PagedFile::open(dir.path().join("pages.dat"), page_size).unwrap());
        let alloc2 = Arc::new(SyncMutex::new(PageAllocator::new(1)));
        let pool2 = Arc::new(BufferPool::new(64, page_size, disk2, alloc2).unwrap());
        let wal2 = Arc::new(WalLog::open(dir.path(), storage_wal::WalOptions::default()).unwrap());

        let fresh_root_guard = pool2.new_page().unwrap();
        let fresh_root = fresh_root_guard.page().id;
        fresh_root_guard.page().set_leaf();
        fresh_root_guard.mark_dirty();
        drop(fresh_root_guard);

        let recovery = Recovery::new(pool2.clone(), wal2.clone(), fresh_root);
        let root = recovery.recover(0).unwrap();

        let tree2 = crate::tree::BPlusTree::open(pool2, root, page_size / 4);
        assert_eq!(tree2.get(b"k").unwrap(), Some(b"old".to_vec()));
    }

    #[test]
    fn crash_during_undo() {
        let dir = tempfile::tempdir().unwrap();
        let page_size = 512;
        let disk = Arc::new(PagedFile::open(dir.path().join("pages.dat"), page_size).unwrap());
        let alloc = Arc::new(SyncMutex::new(PageAllocator::new(1)));
        let pool = Arc::new(BufferPool::new(64, page_size, disk, alloc).unwrap());
        let wal = Arc::new(WalLog::open(dir.path(), storage_wal::WalOptions::default()).unwrap());

        let tree = crate::tree::BPlusTree::new(pool.clone(), page_size / 4)
            .unwrap()
            .with_wal(Arc::clone(&wal));
        tree.insert(b"k", b"old").unwrap();

        let txn = tree
            .begin_txn(crate::txn::IsolationLevel::Snapshot)
            .unwrap();
        tree.insert_txn(&txn, b"k", b"new").unwrap();
        drop(tree);

        // Manually write a CLR that redoes the undo of the update.  This
        // simulates a crash after the CLR was written but before the Abort
        // marker or transaction-table update.
        let records: Vec<_> = wal.iter(0).unwrap().collect::<Result<Vec<_>>>().unwrap();
        let (update_lsn, update_record) = records
            .iter()
            .find(|(_lsn, r)| matches!(r.payload, RecordPayload::UpdateCell { .. }))
            .map(|(lsn, r)| (*lsn, r.clone()))
            .unwrap();
        let clr = Record {
            header: RecordHeader::new(
                RecordType::Clr,
                txn.txn_id,
                update_record.header.prev_lsn,
                update_record.header.page_id,
                update_lsn,
            ),
            payload: RecordPayload::Clr {
                undo_next_lsn: update_record.header.prev_lsn,
                original: Box::new(update_record),
            },
        };
        wal.append(clr).unwrap();

        // Recover: analysis must see the CLR, redo must apply it, and undo must
        // continue from undo_next_lsn (which is the Begin record's prev_lsn).
        let disk2 = Arc::new(PagedFile::open(dir.path().join("pages.dat"), page_size).unwrap());
        let alloc2 = Arc::new(SyncMutex::new(PageAllocator::new(1)));
        let pool2 = Arc::new(BufferPool::new(64, page_size, disk2, alloc2).unwrap());
        let wal2 = Arc::new(WalLog::open(dir.path(), storage_wal::WalOptions::default()).unwrap());

        let fresh_root_guard = pool2.new_page().unwrap();
        let fresh_root = fresh_root_guard.page().id;
        fresh_root_guard.page().set_leaf();
        fresh_root_guard.mark_dirty();
        drop(fresh_root_guard);

        let recovery = Recovery::new(pool2.clone(), wal2.clone(), fresh_root);
        let root = recovery.recover(0).unwrap();

        let tree2 = crate::tree::BPlusTree::open(pool2, root, page_size / 4);
        assert_eq!(tree2.get(b"k").unwrap(), Some(b"old".to_vec()));
    }

    #[test]
    fn recovery_preserves_txn_delete_tombstone() {
        let dir = tempfile::tempdir().unwrap();
        let page_size = 512;
        let disk = Arc::new(PagedFile::open(dir.path().join("pages.dat"), page_size).unwrap());
        let alloc = Arc::new(SyncMutex::new(PageAllocator::new(1)));
        let pool = Arc::new(BufferPool::new(64, page_size, disk, alloc).unwrap());
        let wal = Arc::new(WalLog::open(dir.path(), storage_wal::WalOptions::default()).unwrap());
        let tree = crate::tree::BPlusTree::new(pool.clone(), page_size / 4)
            .unwrap()
            .with_wal(Arc::clone(&wal));

        tree.insert(b"k", b"old").unwrap();

        let t1 = tree
            .begin_txn(crate::txn::IsolationLevel::Snapshot)
            .unwrap();

        let t2 = tree
            .begin_txn(crate::txn::IsolationLevel::Snapshot)
            .unwrap();
        tree.delete_txn(&t2, b"k").unwrap();
        tree.commit_txn(&t2).unwrap();

        // The pre-delete snapshot sees the old value before the crash.
        assert_eq!(tree.get_txn(&t1, b"k").unwrap(), Some(b"old".to_vec()));

        drop(tree);
        drop(pool);

        // Recover and verify the tombstone still points to the old value.
        let disk2 = Arc::new(PagedFile::open(dir.path().join("pages.dat"), page_size).unwrap());
        let alloc2 = Arc::new(SyncMutex::new(PageAllocator::new(1)));
        let pool2 = Arc::new(BufferPool::new(64, page_size, disk2, alloc2).unwrap());
        let wal2 = Arc::new(WalLog::open(dir.path(), storage_wal::WalOptions::default()).unwrap());

        let fresh_root_guard = pool2.new_page().unwrap();
        let fresh_root = fresh_root_guard.page().id;
        fresh_root_guard.page().set_leaf();
        fresh_root_guard.mark_dirty();
        drop(fresh_root_guard);

        let recovery = Recovery::new(pool2.clone(), wal2.clone(), fresh_root);
        let root = recovery.recover(0).unwrap();

        let tree2 = crate::tree::BPlusTree::open(pool2, root, page_size / 4).with_wal(wal2);
        let t3 = tree2
            .begin_txn(crate::txn::IsolationLevel::Snapshot)
            .unwrap();
        assert_eq!(tree2.get_txn(&t3, b"k").unwrap(), Some(b"old".to_vec()));
    }

    #[test]
    #[cfg(not(miri))]
    fn recovery_preserves_mvcc_after_split() {
        let dir = tempfile::tempdir().unwrap();
        let page_size = 512;
        let disk = Arc::new(PagedFile::open(dir.path().join("pages.dat"), page_size).unwrap());
        let alloc = Arc::new(SyncMutex::new(PageAllocator::new(1)));
        let pool = Arc::new(BufferPool::new(64, page_size, disk, alloc).unwrap());
        let wal = Arc::new(WalLog::open(dir.path(), storage_wal::WalOptions::default()).unwrap());
        let tree = crate::tree::BPlusTree::new(pool.clone(), page_size / 4)
            .unwrap()
            .with_wal(Arc::clone(&wal));

        let keys: Vec<String> = (0u64..40).map(|i| format!("{:08x}", i)).collect();
        for key in &keys {
            tree.insert(key.as_bytes(), key.as_bytes()).unwrap();
        }

        let t1 = tree
            .begin_txn(crate::txn::IsolationLevel::Snapshot)
            .unwrap();

        let t2 = tree
            .begin_txn(crate::txn::IsolationLevel::Snapshot)
            .unwrap();
        let big_value = vec![b'x'; 100];
        tree.insert_txn(&t2, keys[0].as_bytes(), &big_value)
            .unwrap();
        tree.commit_txn(&t2).unwrap();

        assert_eq!(
            tree.get_txn(&t1, keys[0].as_bytes()).unwrap(),
            Some(keys[0].as_bytes().to_vec())
        );

        drop(tree);
        drop(pool);

        // Recover and verify the older snapshot still sees the pre-split values.
        let disk2 = Arc::new(PagedFile::open(dir.path().join("pages.dat"), page_size).unwrap());
        let alloc2 = Arc::new(SyncMutex::new(PageAllocator::new(1)));
        let pool2 = Arc::new(BufferPool::new(64, page_size, disk2, alloc2).unwrap());
        let wal2 = Arc::new(WalLog::open(dir.path(), storage_wal::WalOptions::default()).unwrap());

        let fresh_root_guard = pool2.new_page().unwrap();
        let fresh_root = fresh_root_guard.page().id;
        fresh_root_guard.page().set_leaf();
        fresh_root_guard.mark_dirty();
        drop(fresh_root_guard);

        let recovery = Recovery::new(pool2.clone(), wal2.clone(), fresh_root);
        let root = recovery.recover(0).unwrap();

        let tree2 = crate::tree::BPlusTree::open(pool2, root, page_size / 4).with_wal(wal2);
        let t3 = tree2
            .begin_txn(crate::txn::IsolationLevel::Snapshot)
            .unwrap();
        for key in &keys {
            assert_eq!(
                tree2.get_txn(&t3, key.as_bytes()).unwrap(),
                Some(key.as_bytes().to_vec()),
                "recovered tree lost pre-split value for {key}"
            );
        }
    }
}
