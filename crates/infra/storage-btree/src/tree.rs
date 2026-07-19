//! Single-threaded in-place B+ tree.
//!
//! This module implements the core B+ tree algorithms on top of the slotted
//! pages and buffer pool built in Phase 1 and Phase 2.  Concurrency control is
//! intentionally absent here; Phase 4 will replace the frame-level mutex guards
//! with Optimistic Lock Coupling.
//!
//! Layout:
//! * Leaf pages store key/value cells in sorted order and are linked by
//!   `prev_page_id` / `next_page_id`.
//! * Internal pages store a `leftmost_child` page id plus cells of the form
//!   `(separator_key, right_child_page_id)`.
//! * The separator key for an internal cell is the smallest key stored in the
//!   subtree pointed to by `right_child_page_id`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::sync::Mutex as SyncMutex;

use crate::buffer::{BufferPool, PageGuard};
use crate::error::{Error, Result};
use crate::page::{NULL_PAGE_ID, PageId, WriteGuard};
use crate::slot::{OwnedCell, OwnedValue, Slot, ValueKind};
use crate::txn::{
    IsolationLevel, NULL_TXN_ID, Timestamp, Transaction, TransactionTable, TxnId, TxnOracle,
};
use crate::valuelog::{ValueLen, ValueLog, ValueOffset};
use crate::version::{MvccHeader, VisibleValue, resolve_version_chain};
use crate::wal::{
    Lsn, NULL_LSN, Record, RecordHeader, RecordPayload, RecordType, WalLog, page_delete_logged,
    page_delete_txn_logged, page_insert_logged, page_insert_txn_logged, page_move_cell_logged,
    page_replace_key_logged, page_set_leftmost_child_logged, page_update_txn_logged,
};

/// Default minimum number of live cells a non-root page must retain.  Pages
/// with more cells than this can donate one during redistribution.
const DEFAULT_MIN_CELLS: usize = 1;

/// Result of a page split: the separator key and the new right page.
#[derive(Clone, Debug)]
struct SplitResult {
    separator: Vec<u8>,
    right_page_id: PageId,
}

/// Result of an optimistic root-to-leaf traversal.
struct OptimisticLeaf {
    /// Guard pinning the leaf frame.
    guard: PageGuard,
    /// Root page id observed at the start of the traversal.
    root_id: PageId,
    /// Parent guards captured during descent, each paired with the latch
    /// version observed at that level.  Ordered root -> parent.
    path: Vec<(PageGuard, u64)>,
}

impl OptimisticLeaf {
    /// True if every captured parent version is still current and each parent
    /// still points to the next page on the path for `key`.
    fn path_valid(&self, key: &[u8]) -> bool {
        let mut expected_child = self.guard.page().id;
        for (g, version) in self.path.iter().rev() {
            let page = g.page();
            if page.latch_word() != *version {
                return false;
            }
            match child_for_key(page, key) {
                Ok(id) if id == expected_child => {}
                _ => return false,
            }
            expected_child = page.id;
        }
        true
    }
}

/// Pinned path captured during an optimistic traversal.  `leaf_arc` provides
/// cheap shared access for locking and validation; `leaf_guard` keeps the leaf
/// frame pinned so eviction cannot replace it while the caller holds the leaf
/// write latch.
struct OptimisticPath {
    leaf_arc: Arc<crate::page::Page>,
    /// Keeps the leaf frame pinned while the caller holds the leaf write latch.
    /// The field is never read directly; it exists for its `Drop` side effect.
    #[allow(dead_code)]
    leaf_guard: PageGuard,
    /// Root page id observed at the start of the traversal.
    root_id: PageId,
    path: Vec<(PageGuard, Arc<crate::page::Page>, u64)>,
}

impl OptimisticPath {
    /// True if every captured ancestor version is still current and each
    /// ancestor still points to the next page on the path for `key`.
    fn path_valid(&self, key: &[u8]) -> bool {
        Self::path_valid_for(&self.path, key, self.leaf_arc.id)
    }

    fn path_valid_for(
        path: &[(PageGuard, Arc<crate::page::Page>, u64)],
        key: &[u8],
        leaf_id: PageId,
    ) -> bool {
        let mut expected_child = leaf_id;
        for (_, arc, version) in path.iter().rev() {
            if arc.latch_word() != *version {
                return false;
            }
            match child_for_key(arc, key) {
                Ok(id) if id == expected_child => {}
                _ => return false,
            }
            expected_child = arc.id;
        }
        true
    }
}

/// A single-threaded in-place B+ tree.
pub struct BPlusTree {
    pool: Arc<BufferPool>,
    root_page_id: AtomicU64,
    /// Maximum inline value size; larger values are stored in the value log.
    inline_threshold: usize,
    /// Minimum number of live cells a page must retain; below this the page
    /// tries to redistribute or merge.
    min_cells: usize,
    /// Page ids that are no longer reachable from the current root but could
    /// not be freed immediately because another thread still held a pin on the
    /// frame.  Reclaimed opportunistically after structure modifications.
    retired: SyncMutex<Vec<PageId>>,
    /// Roots currently pinned by active cursors or transactions.  A non-zero
    /// count prevents `compact` from reclaiming pages reachable from that root.
    active_roots: SyncMutex<HashMap<PageId, usize>>,
    /// Optional physiological WAL.  When present, every modifying operation
    /// appends records before changing pages and updates `page_lsn`.
    wal: Option<Arc<WalLog>>,
    /// In-memory transaction table.  Used to assign txn ids / read timestamps
    /// and to resolve version visibility.
    txn_table: Arc<TransactionTable>,
    /// Optional append-only value log for values larger than
    /// `inline_threshold`.
    value_log: Option<Arc<ValueLog>>,
}

impl BPlusTree {
    /// Create a new empty tree backed by `pool`.
    pub fn new(pool: Arc<BufferPool>, inline_threshold: usize) -> Result<Self> {
        let root = pool.new_page()?;
        let root_id = root.page().id;
        drop(root);
        let root = pool.fetch_or_read(root_id)?;
        root.page().set_leaf();
        root.mark_dirty();
        drop(root);
        Ok(Self {
            pool,
            root_page_id: AtomicU64::new(root_id.get()),
            inline_threshold,
            min_cells: DEFAULT_MIN_CELLS,
            retired: SyncMutex::new(Vec::new()),
            active_roots: SyncMutex::new(HashMap::new()),
            wal: None,
            txn_table: Arc::new(TransactionTable::new()),
            value_log: None,
        })
    }

    /// Open an existing tree backed by `pool` with the given root page id.
    ///
    /// The caller is responsible for ensuring `root_page_id` is valid and that
    /// recovery, if needed, has already been run.
    pub fn open(pool: Arc<BufferPool>, root_page_id: PageId, inline_threshold: usize) -> Self {
        Self::open_with_txn_table(pool, root_page_id, inline_threshold, Arc::new(TransactionTable::new()))
    }

    /// Open an existing tree with an explicit transaction table.
    ///
    /// This is used by the engine after recovery so that committed transactions
    /// discovered in the WAL are visible to MVCC readers.
    pub fn open_with_txn_table(
        pool: Arc<BufferPool>,
        root_page_id: PageId,
        inline_threshold: usize,
        txn_table: Arc<TransactionTable>,
    ) -> Self {
        Self {
            pool,
            root_page_id: AtomicU64::new(root_page_id.get()),
            inline_threshold,
            min_cells: DEFAULT_MIN_CELLS,
            retired: SyncMutex::new(Vec::new()),
            active_roots: SyncMutex::new(HashMap::new()),
            wal: None,
            txn_table,
            value_log: None,
        }
    }

    /// Create a tree with an explicit minimum cell count, useful in tests with
    /// very small pages.
    #[cfg(test)]
    fn with_min_cells(
        pool: Arc<BufferPool>,
        inline_threshold: usize,
        min_cells: usize,
    ) -> Result<Self> {
        Self::new_with_min_cells(pool, inline_threshold, min_cells)
    }

    /// Create a tree with an explicit minimum cell count.
    pub fn new_with_min_cells(
        pool: Arc<BufferPool>,
        inline_threshold: usize,
        min_cells: usize,
    ) -> Result<Self> {
        let root = pool.new_page()?;
        let root_id = root.page().id;
        drop(root);
        let root = pool.fetch_or_read(root_id)?;
        root.page().set_leaf();
        root.mark_dirty();
        drop(root);
        Ok(Self {
            pool,
            root_page_id: AtomicU64::new(root_id.get()),
            inline_threshold,
            min_cells,
            retired: SyncMutex::new(Vec::new()),
            active_roots: SyncMutex::new(HashMap::new()),
            wal: None,
            txn_table: Arc::new(TransactionTable::new()),
            value_log: None,
        })
    }

    /// Attach a physiological WAL to this tree.  All subsequent modifying
    /// operations are logged.
    pub fn with_wal(mut self, wal: Arc<WalLog>) -> Self {
        self.wal = Some(wal);
        self
    }

    /// Attach an append-only value log to this tree.  Values larger than
    /// `inline_threshold` will be stored in the log.
    pub fn with_value_log(mut self, value_log: Arc<ValueLog>) -> Self {
        self.value_log = Some(value_log);
        self
    }

    /// Return a reference to the underlying buffer pool.
    pub(crate) fn pool(&self) -> &Arc<BufferPool> {
        &self.pool
    }

    /// Return a borrowed view of the attached WAL, if any.
    fn wal(&self) -> Option<&WalLog> {
        self.wal.as_deref()
    }

    /// Return a borrowed view of the attached value log, if any.
    pub(crate) fn value_log(&self) -> Option<&ValueLog> {
        self.value_log.as_deref()
    }

    /// Flush and fsync the WAL and value log.
    ///
    /// This must be called after a sequence of buffered appends to make the
    /// mutation durable. It is the caller's responsibility to ensure that all
    /// records for a single logical operation are synced together.
    pub(crate) fn sync(&self) -> Result<()> {
        // Sync the value log before the WAL: commit records in the WAL may
        // reference value-log offsets, so the values must be durable first.
        if let Some(vl) = self.value_log() {
            vl.sync()?;
        }
        if let Some(wal) = self.wal() {
            wal.sync()?;
        }
        Ok(())
    }

    /// Build a [`ValueKind`] for `value`, appending to the value log if it is
    /// larger than the inline threshold.
    fn prepare_value<'a>(&self, value: &'a [u8]) -> Result<ValueKind<'a>> {
        if value.len() > self.inline_threshold {
            let vl = self
                .value_log()
                .ok_or(Error::Unsupported("value log not configured"))?;
            let (offset, len) = vl.append(value)?;
            vl.add_ref(offset, len);
            Ok(ValueKind::ValueLog { offset, len })
        } else {
            Ok(ValueKind::Inline(value))
        }
    }

    /// Release a value-log reference held by an overwritten or deleted cell.
    fn release_value(&self, value: &OwnedValue) {
        if let OwnedValue::ValueLog { offset, len } = value
            && let Some(vl) = self.value_log()
        {
            vl.release(*offset, *len);
        }
    }

    /// Add a value-log reference for a cell that is becoming reachable again.
    fn add_value_ref(&self, value: &OwnedValue) {
        if let OwnedValue::ValueLog { offset, len } = value
            && let Some(vl) = self.value_log()
        {
            vl.add_ref(*offset, *len);
        }
    }

    /// Compact the value log and update all leaf-cell references.
    ///
    /// This is a stop-the-world operation: it GCs the value log and then scans
    /// every leaf page, rewriting cells whose value-log offsets moved.  Returns
    /// the old-to-new offset mapping produced by the GC.
    ///
    /// # Limitations
    ///
    /// WAL undo records are not rewritten, so this must not be called while an
    /// old snapshot may need to follow a version chain back to a value-log
    /// entry that could be moved.  In practice, run this only when all active
    /// transactions began after the last GC.
    pub fn compact_value_log(&self) -> Result<HashMap<(ValueOffset, ValueLen), ValueOffset>> {
        let value_log = match self.value_log() {
            Some(vl) => vl,
            None => return Ok(HashMap::new()),
        };
        let mapping = value_log.gc()?;
        if mapping.is_empty() {
            return Ok(HashMap::new());
        }
        let lookup: HashMap<(ValueOffset, ValueLen), ValueOffset> =
            mapping.iter().copied().collect();

        // Find the leftmost leaf by descending leftmost children from the root.
        let mut current_id = self.root_page_id();
        loop {
            let guard = self.pool.fetch_or_read(current_id)?;
            let page = guard.page();
            if page.is_leaf() {
                break;
            }
            current_id = page.leftmost_child()?;
        }

        // Walk the leaf sibling chain and rewrite stale value-log references.
        // Each leaf is locked exclusively while it is processed and its next
        // pointer is read, so concurrent structure modifications cannot move
        // the chain out from under us.
        while current_id != NULL_PAGE_ID {
            let guard = self.pool.fetch_or_read(current_id)?;
            let arc = guard.page_arc();
            let write = match arc.try_write() {
                Some(w) => w,
                None => {
                    // A concurrent writer is modifying this leaf.  Yield and
                    // retry the same page id; the tree structure is stable.
                    std::thread::yield_now();
                    continue;
                }
            };
            let page = write.page();
            let count = page.slot_count()?;
            for idx in 0..count {
                if page.read_slot(idx)?.is_deleted() {
                    continue;
                }
                let cell = page.get_by_slot(idx)?;
                if let OwnedValue::ValueLog { offset, len } = cell.value
                    && let Some(&new_offset) = lookup.get(&(offset, len))
                {
                    let new_value = ValueKind::ValueLog {
                        offset: new_offset,
                        len,
                    };
                    page.insert_with_mvcc(&cell.key, &new_value, cell.mvcc.as_ref())?;
                }
            }
            self.pool.mark_dirty(current_id)?;
            current_id = page.next_page_id()?;
        }

        // Make the rewritten leaf pages and value-log GC durable.
        self.sync()?;

        Ok(lookup)
    }

    /// Log a page split and update `page_lsn` on both pages.
    fn log_split(
        &self,
        left: &crate::page::Page,
        right: &crate::page::Page,
        separator: &[u8],
        is_internal: bool,
    ) -> Result<Lsn> {
        let page_lsn = left.header()?.page_lsn;
        let record = Record {
            header: RecordHeader::new(
                crate::wal::RecordType::SplitPage,
                NULL_TXN_ID,
                NULL_LSN,
                left.id,
                page_lsn,
            ),
            payload: RecordPayload::SplitPage {
                separator: separator.to_vec(),
                right_page_id: right.id,
                is_internal,
            },
        };
        let lsn = match self.wal() {
            Some(wal) => wal.append(record)?,
            None => NULL_LSN,
        };
        crate::wal::set_page_lsn(left, lsn)?;
        crate::wal::set_page_lsn(right, lsn)?;
        Ok(lsn)
    }

    /// Log a root change.
    fn log_set_root(&self, new_root_page_id: PageId) -> Result<Lsn> {
        let record = Record {
            header: RecordHeader::new(
                crate::wal::RecordType::SetRoot,
                NULL_TXN_ID,
                NULL_LSN,
                NULL_PAGE_ID,
                NULL_LSN,
            ),
            payload: RecordPayload::SetRoot { new_root_page_id },
        };
        match self.wal() {
            Some(wal) => wal.append(record),
            None => Ok(NULL_LSN),
        }
    }

    /// Log the creation of a brand-new root page during a root split.
    ///
    /// This record fully describes the initial content of the new root so that
    /// recovery can re-create it even if it was allocated but never flushed.
    fn log_new_root(
        &self,
        new_root: &crate::page::Page,
        leftmost_child: PageId,
        separator: &[u8],
        right_child: PageId,
    ) -> Result<Lsn> {
        let record = Record {
            header: RecordHeader::new(
                crate::wal::RecordType::NewRoot,
                NULL_TXN_ID,
                NULL_LSN,
                new_root.id,
                NULL_LSN,
            ),
            payload: RecordPayload::NewRoot {
                new_root_page_id: new_root.id,
                leftmost_child,
                separator: separator.to_vec(),
                right_child,
            },
        };
        let lsn = match self.wal() {
            Some(wal) => wal.append(record)?,
            None => NULL_LSN,
        };
        crate::wal::set_page_lsn(new_root, lsn)?;
        Ok(lsn)
    }

    /// Log a page merge.  The surviving page is `survivor`; `victim_page_id` is
    /// the page being retired.  For internal merges `separator` and
    /// `victim_leftmost` describe the parent separator and the victim's old
    /// leftmost child; for leaf merges both are unused.
    fn log_merge(
        &self,
        survivor: &crate::page::Page,
        victim_page_id: PageId,
        victim_is_left: bool,
        separator: &[u8],
        victim_leftmost: PageId,
    ) -> Result<Lsn> {
        let page_lsn = survivor.header()?.page_lsn;
        let record = Record {
            header: RecordHeader::new(
                crate::wal::RecordType::MergePage,
                NULL_TXN_ID,
                NULL_LSN,
                survivor.id,
                page_lsn,
            ),
            payload: RecordPayload::MergePage {
                victim_page_id,
                victim_is_left,
                separator: separator.to_vec(),
                victim_leftmost,
            },
        };
        let lsn = match self.wal() {
            Some(wal) => wal.append(record)?,
            None => NULL_LSN,
        };
        crate::wal::set_page_lsn(survivor, lsn)?;
        Ok(lsn)
    }

    /// Log a change to an internal page's rightmost child pointer (used during
    /// root shrink to poison the old root's leftmost child).
    fn log_move_rightmost(
        &self,
        page: &crate::page::Page,
        old_rightmost: PageId,
        new_rightmost: PageId,
    ) -> Result<Lsn> {
        let page_lsn = page.header()?.page_lsn;
        let record = Record {
            header: RecordHeader::new(
                crate::wal::RecordType::MoveRightmost,
                NULL_TXN_ID,
                NULL_LSN,
                page.id,
                page_lsn,
            ),
            payload: RecordPayload::MoveRightmost {
                old_rightmost,
                new_rightmost,
            },
        };
        let lsn = match self.wal() {
            Some(wal) => wal.append(record)?,
            None => NULL_LSN,
        };
        crate::wal::set_page_lsn(page, lsn)?;
        Ok(lsn)
    }

    /// Return the current root page id.
    pub fn root_page_id(&self) -> PageId {
        PageId::new(self.root_page_id.load(Ordering::SeqCst))
    }

    fn load_root(&self) -> PageId {
        PageId::new(self.root_page_id.load(Ordering::Acquire))
    }

    fn store_root(&self, id: PageId) {
        self.root_page_id.store(id.get(), Ordering::SeqCst);
    }

    /// Return the current global timestamp.
    ///
    /// This is useful for cursors and autocommit reads that want a stable
    /// snapshot without registering a full transaction in the transaction table.
    pub fn current_timestamp(&self) -> Timestamp {
        self.txn_table.current_timestamp()
    }

    /// Set the minimum live cell count used to decide when a page underflows.
    pub fn set_min_cells(&mut self, min_cells: usize) {
        self.min_cells = min_cells.max(1);
    }

    /// Record `page_id` as no longer reachable from the current root.
    ///
    /// The page is added to the retired list; it is not freed immediately so
    /// that cursors or transactions that pinned an older root can still reach
    /// it safely.  Call `compact` to reclaim retired pages that are no longer
    /// reachable from any pinned root.
    fn retire_page(&self, page_id: PageId) -> Result<()> {
        self.retired.with_mut(|retired| {
            retired.push(page_id);
            Ok(())
        })
    }

    /// Number of page ids currently waiting to be reclaimed by `compact`.
    pub fn retired_count(&self) -> usize {
        self.retired.with_mut(|r| r.len())
    }

    /// Highest page id reachable from any pinned root plus the current root.
    ///
    /// Used by file shrink to avoid truncating pages that are still reachable
    /// from active cursors or snapshots.
    pub fn highest_rooted_page_id(&self) -> u64 {
        let current_root = self.load_root();
        let mut max = current_root.get();
        self.active_roots.with_mut(|roots| {
            for &root in roots.keys() {
                if root.get() > max {
                    max = root.get();
                }
            }
        });
        max
    }

    /// Pin `root` so that `compact` will not reclaim pages reachable from it.
    ///
    /// Each call to `pin_root` must be paired with a later `unpin_root`.
    pub fn pin_root(&self, root: PageId) {
        if root == NULL_PAGE_ID {
            return;
        }
        self.active_roots.with_mut(|roots| {
            *roots.entry(root).or_insert(0) += 1;
        });
    }

    /// Unpin a root previously pinned by `pin_root`.
    pub fn unpin_root(&self, root: PageId) {
        if root == NULL_PAGE_ID {
            return;
        }
        self.active_roots.with_mut(|roots| {
            if let std::collections::hash_map::Entry::Occupied(mut entry) = roots.entry(root) {
                *entry.get_mut() -= 1;
                if *entry.get() == 0 {
                    entry.remove();
                }
            }
        });
    }

    /// Return the set of page ids reachable from `root`.
    fn reachable_pages(&self, root: PageId) -> Result<HashSet<PageId>> {
        let mut seen = HashSet::new();
        self.reachable_pages_recursive(root, &mut seen)?;
        Ok(seen)
    }

    fn reachable_pages_recursive(&self, page_id: PageId, seen: &mut HashSet<PageId>) -> Result<()> {
        if page_id == NULL_PAGE_ID || !seen.insert(page_id) {
            return Ok(());
        }
        let guard = self.pool.fetch_or_read(page_id)?;
        let page = guard.page();
        if page.is_leaf() {
            // Include sibling pointers so that a cursor walking a pinned leaf
            // chain cannot see a freed page.
            if let Ok(next) = page.next_page_id() {
                self.reachable_pages_recursive(next, seen)?;
            }
            if let Ok(prev) = page.prev_page_id() {
                self.reachable_pages_recursive(prev, seen)?;
            }
            return Ok(());
        }
        self.reachable_pages_recursive(page.leftmost_child()?, seen)?;
        for idx in 0..page.slot_count()? {
            if page.read_slot(idx)?.is_deleted() {
                continue;
            }
            let cell = page.get_by_slot(idx)?;
            let child_id = decode_page_id(&cell.value.as_value_kind())?;
            self.reachable_pages_recursive(child_id, seen)?;
        }
        Ok(())
    }

    /// Reclaim retired pages that are no longer reachable from the current
    /// root or from any pinned root.
    ///
    /// Pages whose frames are still pinned are left in the retired list for
    /// the next reclamation pass.
    pub fn compact(&self) -> Result<()> {
        let current_root = self.root_page_id();

        let pinned: Vec<PageId> = self
            .active_roots
            .with_mut(|roots| roots.keys().copied().collect());

        let mut live = HashSet::new();
        if current_root != NULL_PAGE_ID {
            live.extend(self.reachable_pages(current_root)?);
        }
        for root in pinned {
            if root != current_root && root != NULL_PAGE_ID && !live.contains(&root) {
                live.extend(self.reachable_pages(root)?);
            }
        }

        self.retired.with_mut(|retired| {
            let mut still_retired: Vec<PageId> = Vec::new();
            for id in retired.drain(..) {
                if live.contains(&id) {
                    still_retired.push(id);
                    continue;
                }
                match self.pool.free_page(id) {
                    Ok(()) => {}
                    Err(Error::Corruption(msg)) if msg.contains("still pinned") => {
                        still_retired.push(id);
                    }
                    Err(e) => return Err(e),
                }
            }
            retired.extend(still_retired);
            Ok(())
        })
    }

    #[cfg(debug_assertions)]
    fn debug_leaf_for_key(&self, key: &[u8]) -> Result<PageId> {
        let mut current_id = self.root_page_id();
        loop {
            let guard = self.pool.fetch_or_read(current_id)?;
            let page = guard.page();
            if page.is_leaf() {
                return Ok(current_id);
            }
            current_id = child_for_key(page, key)?;
        }
    }

    /// Look up `key` and return the owned inline value if found.
    ///
    /// Reads use Optimistic Lock Coupling: the traversal snapshots latch
    /// versions on the root-to-leaf path and validates them after reading the
    /// leaf.  If any page changed mid-traversal the operation retries from the
    /// root.
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        loop {
            match self.try_get(key)? {
                Some(value) => return Ok(value),
                None => continue,
            }
        }
    }

    fn try_get(&self, key: &[u8]) -> Result<Option<Option<Vec<u8>>>> {
        let leaf = match self.optimistic_leaf(key)? {
            Some(l) => l,
            None => return Ok(None),
        };

        // A root split may have changed the root pointer since we started the
        // traversal.  If so, the captured path is stale and we must retry.
        if self.load_root() != leaf.root_id {
            return Ok(None);
        }

        let page = leaf.guard.page();
        let opt = match page.optimistic() {
            Some(o) => o,
            None => return Ok(None),
        };

        let result = opt.read(|p| -> Result<Option<Vec<u8>>> {
            let opt_cell = p.get(key)?;
            Ok(if let Some(c) = opt_cell {
                match c.value.as_value_kind() {
                    ValueKind::Inline(v) => Some(v.to_vec()),
                    ValueKind::Tombstone => None,
                    ValueKind::ValueLog { offset, len } => {
                        let bytes = self
                            .value_log()
                            .expect("value log missing")
                            .read(offset, len)?;
                        Some(bytes)
                    }
                }
            } else {
                None
            })
        });

        match result {
            None => Ok(None),
            Some(Err(e)) => Err(e),
            Some(Ok(value)) => {
                // Validate the captured root-to-leaf path.  The leaf itself is
                // already validated by `opt.read`; re-check the parents.
                if !leaf.path_valid(key) {
                    return Ok(None);
                }
                Ok(Some(value))
            }
        }
    }

    /// Optimistically descend from the current root to the leaf that should
    /// contain `key`.  Returns `None` when a page version changes during the
    /// descent (caller retries).
    fn optimistic_leaf(&self, key: &[u8]) -> Result<Option<OptimisticLeaf>> {
        let root_id = self.load_root();
        let mut path: Vec<(PageGuard, u64)> = Vec::new();
        let mut current_id = root_id;

        loop {
            let guard = self.pool.fetch_or_read(current_id)?;
            let page = guard.page();
            let version = match page.optimistic_version() {
                Some(v) => v,
                // A locked page means an in-flight writer; retry from the root.
                None => return Ok(None),
            };

            if page.is_leaf() {
                return Ok(Some(OptimisticLeaf {
                    guard,
                    root_id,
                    path,
                }));
            }

            let child_id = child_for_key(page, key)?;
            if page.latch_word() != version {
                return Ok(None);
            }

            path.push((guard, version));
            current_id = child_id;
        }
    }

    /// Insert or replace `value` for `key`.
    ///
    /// Writes use Optimistic Lock Coupling: the traversal descends with
    /// optimistic reads while keeping a pinned path, locks the leaf, validates
    /// the path, and mutates.  Structure modifications (splits) lock the parent
    /// while holding the child, top-down, without a global writer lock.
    pub fn insert(&self, key: &[u8], value: &[u8]) -> Result<()> {
        let value_kind = self.prepare_value(value)?;
        loop {
            match self.lock_coupled_insert(key, &value_kind)? {
                Some(old_value) => {
                    if let Some(old) = old_value {
                        self.release_value(&old);
                    }
                    // Buffered WAL/value-log appends are now durable for this
                    // autocommit operation.
                    self.sync()?;
                    #[cfg(debug_assertions)]
                    {
                        let found = self.get(key)?;
                        if found.as_deref() != Some(value) {
                            let routed = self.debug_leaf_for_key(key)?;
                            eprintln!(
                                "DBG missing key {:?} routed to leaf {} root {}",
                                key,
                                routed,
                                self.root_page_id()
                            );
                        }
                        assert_eq!(
                            found.as_deref(),
                            Some(value),
                            "key {:?} not readable after insert",
                            key
                        );
                    }
                    return Ok(());
                }
                None => continue,
            }
        }
    }

    fn lock_coupled_insert(
        &self,
        key: &[u8],
        value: &ValueKind<'_>,
    ) -> Result<Option<Option<OwnedValue>>> {
        let target = match self.optimistic_path_to_leaf(key)? {
            Some(t) => t,
            None => return Ok(None),
        };

        // Clone the leaf `Arc` so the write guard borrows a local copy; this
        // lets us move `target` (and its pinned guard) into the split path.
        let leaf_arc = Arc::clone(&target.leaf_arc);
        let leaf_write = match leaf_arc.try_write() {
            Some(w) => w,
            None => return Ok(None),
        };

        // A root split may have changed the root pointer since we started the
        // traversal.  If so, the captured path is stale and we must retry.
        if self.load_root() != target.root_id {
            return Ok(None);
        }
        // Validate the captured root-to-leaf path.
        if !target.path_valid(key) {
            return Ok(None);
        }

        let old_cell = leaf_write.page().get(key)?;
        match page_insert_logged(leaf_write.page(), self.wal(), key, value) {
            Ok(_) => {
                self.pool.mark_dirty(leaf_arc.id)?;
                return Ok(Some(old_cell.map(|c| c.value)));
            }
            Err(Error::PageFull) => {}
            Err(e) => return Err(e),
        }

        // Root-leaf split: the leaf is the only page in the tree, so we grow a
        // new root above it.
        if target.path.is_empty() {
            return self.split_root_leaf_locked(leaf_arc.id, leaf_write, key, value);
        }

        // The leaf is full and has a parent.  To avoid publishing a torn
        // structure, we lock the entire root-to-leaf path before modifying any
        // page.  Modifications are then applied bottom-up while all involved
        // latches are held.  Locking the whole path is conservative; once the
        // protocol is proven we can narrow the lock set to the suffix that
        // actually needs to split.
        let path = target.path;
        let mut locked_arcs: Vec<Arc<crate::page::Page>> = Vec::with_capacity(path.len() + 1);
        let mut ancestor_versions: Vec<u64> = Vec::with_capacity(path.len());
        locked_arcs.push(Arc::clone(&leaf_arc));
        for (_, arc, version) in path.iter().rev() {
            locked_arcs.push(Arc::clone(arc));
            ancestor_versions.push(*version);
        }

        let mut locked: Vec<WriteGuard<'_>> = Vec::with_capacity(locked_arcs.len());
        locked.push(leaf_write);
        for i in 1..locked_arcs.len() {
            let write = match locked_arcs[i].try_write() {
                Some(w) => w,
                None => return Ok(None),
            };
            // The captured version must still be current (lock bit stripped).
            if locked_arcs[i].latch_word() & !1 != ancestor_versions[i - 1] {
                return Ok(None);
            }
            locked.push(write);
        }

        // Split the leaf and insert the new key/value into the correct half.
        let right_guard = self.pool.new_page()?;
        let right_arc = right_guard.page_arc();
        let right_write = match right_arc.try_write() {
            Some(w) => w,
            None => return Ok(None),
        };
        right_write.page().set_leaf();

        let leaf_id = locked_arcs[0].id;
        let separator = split_leaf(locked[0].page(), right_write.page())?;
        self.log_split(locked[0].page(), right_write.page(), &separator, false)?;
        let old_cell = locked[0].page().get(key)?;
        if key >= separator.as_slice() {
            page_insert_logged(right_write.page(), self.wal(), key, value)?;
        } else {
            page_insert_logged(locked[0].page(), self.wal(), key, value)?;
        }
        self.pool.mark_dirty(leaf_id)?;
        self.pool.mark_dirty(right_arc.id)?;

        self.link_siblings_after_split_locked(
            leaf_id,
            locked[0].page(),
            right_arc.id,
            right_write.page(),
        )?;

        let mut split = SplitResult {
            separator,
            right_page_id: right_arc.id,
        };

        // Propagate the split upward through the pre-locked ancestors, stopping
        // before the root.  The root is handled separately because a root split
        // also grows a new root page.
        for parent_write in locked.iter().take(locked.len().saturating_sub(1)).skip(1) {
            let parent_id = parent_write.page().id;
            let child_bytes = encode_page_id(split.right_page_id);

            match page_insert_logged(
                parent_write.page(),
                self.wal(),
                &split.separator,
                &ValueKind::Inline(&child_bytes),
            ) {
                Ok(_) => {
                    self.pool.mark_dirty(parent_id)?;
                    return Ok(Some(None));
                }
                Err(Error::PageFull) => {
                    // The parent is full too.  Split it and carry the promoted
                    // separator to the next ancestor (which is already locked).
                    let new_right_guard = self.pool.new_page()?;
                    let new_right_arc = new_right_guard.page_arc();
                    let new_right_write = match new_right_arc.try_write() {
                        Some(w) => w,
                        None => return Ok(None),
                    };
                    new_right_write.page().set_internal();

                    let promoted = split_internal(parent_write.page(), new_right_write.page())?;
                    self.log_split(parent_write.page(), new_right_write.page(), &promoted, true)?;
                    if split.separator.as_slice() > promoted.as_slice() {
                        page_insert_logged(
                            new_right_write.page(),
                            self.wal(),
                            &split.separator,
                            &ValueKind::Inline(&child_bytes),
                        )?;
                    } else {
                        page_insert_logged(
                            parent_write.page(),
                            self.wal(),
                            &split.separator,
                            &ValueKind::Inline(&child_bytes),
                        )?;
                    }
                    self.pool.mark_dirty(parent_id)?;
                    self.pool.mark_dirty(new_right_arc.id)?;

                    split = SplitResult {
                        separator: promoted,
                        right_page_id: new_right_arc.id,
                    };
                }
                Err(e) => return Err(e),
            }
        }

        // Handle the root.  If it has space we insert the pending separator;
        // otherwise we split the root and grow a new root above it.
        let root_write = locked
            .last()
            .expect("locked path contains at least the leaf");
        let root_id = root_write.page().id;
        let child_bytes = encode_page_id(split.right_page_id);

        match page_insert_logged(
            root_write.page(),
            self.wal(),
            &split.separator,
            &ValueKind::Inline(&child_bytes),
        ) {
            Ok(_) => {
                self.pool.mark_dirty(root_id)?;
                return Ok(Some(None));
            }
            Err(Error::PageFull) => {}
            Err(e) => return Err(e),
        }

        let new_right_guard = self.pool.new_page()?;
        let new_right_arc = new_right_guard.page_arc();
        let new_right_write = match new_right_arc.try_write() {
            Some(w) => w,
            None => return Ok(None),
        };
        new_right_write.page().set_internal();

        let promoted = split_internal(root_write.page(), new_right_write.page())?;
        self.log_split(root_write.page(), new_right_write.page(), &promoted, true)?;
        if split.separator.as_slice() > promoted.as_slice() {
            page_insert_logged(
                new_right_write.page(),
                self.wal(),
                &split.separator,
                &ValueKind::Inline(&child_bytes),
            )?;
        } else {
            page_insert_logged(
                root_write.page(),
                self.wal(),
                &split.separator,
                &ValueKind::Inline(&child_bytes),
            )?;
        }
        self.pool.mark_dirty(root_id)?;
        self.pool.mark_dirty(new_right_arc.id)?;

        let new_root_guard = self.pool.new_page()?;
        let new_root_arc = new_root_guard.page_arc();
        let new_root_write = match new_root_arc.try_write() {
            Some(w) => w,
            None => return Ok(None),
        };
        new_root_write.page().set_internal();
        self.log_new_root(new_root_write.page(), root_id, &promoted, new_right_arc.id)?;
        new_root_write.page().set_leftmost_child(root_id);
        let bytes = encode_page_id(new_right_arc.id);
        new_root_write
            .page()
            .insert(&promoted, &ValueKind::Inline(&bytes))?;
        self.pool.mark_dirty(new_root_arc.id)?;

        self.log_set_root(new_root_arc.id)?;
        self.store_root(new_root_arc.id);
        Ok(Some(old_cell.map(|c| c.value)))
    }

    fn split_root_leaf_locked(
        &self,
        root_id: PageId,
        root_write: WriteGuard<'_>,
        key: &[u8],
        value: &ValueKind<'_>,
    ) -> Result<Option<Option<OwnedValue>>> {
        let right_guard = self.pool.new_page()?;
        let right_arc = right_guard.page_arc();
        let right_write = match right_arc.try_write() {
            Some(w) => w,
            None => return Ok(None),
        };
        right_write.page().set_leaf();

        let separator = split_leaf(root_write.page(), right_write.page())?;
        self.log_split(root_write.page(), right_write.page(), &separator, false)?;
        let old_cell = root_write.page().get(key)?;
        if key >= separator.as_slice() {
            page_insert_logged(right_write.page(), self.wal(), key, value)?;
        } else {
            page_insert_logged(root_write.page(), self.wal(), key, value)?;
        }
        self.pool.mark_dirty(root_id)?;
        self.pool.mark_dirty(right_arc.id)?;

        self.link_siblings_after_split_locked(
            root_id,
            root_write.page(),
            right_arc.id,
            right_write.page(),
        )?;

        let new_root_guard = self.pool.new_page()?;
        let new_root_arc = new_root_guard.page_arc();
        let new_root_write = match new_root_arc.try_write() {
            Some(w) => w,
            None => return Ok(None),
        };
        new_root_write.page().set_internal();
        self.log_new_root(new_root_write.page(), root_id, &separator, right_arc.id)?;
        new_root_write.page().set_leftmost_child(root_id);
        let bytes = encode_page_id(right_arc.id);
        new_root_write
            .page()
            .insert(&separator, &ValueKind::Inline(&bytes))?;
        self.pool.mark_dirty(new_root_arc.id)?;

        self.log_set_root(new_root_arc.id)?;
        self.store_root(new_root_arc.id);
        Ok(Some(old_cell.map(|c| c.value)))
    }

    /// Optimistically descend from the current root to the leaf that should
    /// contain `key`, keeping a pinned path of ancestors with their latch
    /// versions.  Returns `None` if a page changes during descent.
    fn optimistic_path_to_leaf(&self, key: &[u8]) -> Result<Option<OptimisticPath>> {
        let root_id = self.load_root();
        let mut path: Vec<(PageGuard, Arc<crate::page::Page>, u64)> = Vec::new();
        let mut current_id = root_id;

        loop {
            let guard = self.pool.fetch_or_read(current_id)?;
            let arc = guard.page_arc();
            let version = match arc.optimistic_version() {
                Some(v) => v,
                None => return Ok(None),
            };

            if arc.is_leaf() {
                return Ok(Some(OptimisticPath {
                    leaf_arc: arc,
                    leaf_guard: guard,
                    root_id,
                    path,
                }));
            }

            let child_id = child_for_key(&arc, key)?;
            if arc.latch_word() != version {
                return Ok(None);
            }
            // A poisoned old root (or any transiently invalid internal node) may
            // have a null child pointer.  Treat it like a concurrent structure
            // modification and retry from the current root.
            if child_id == NULL_PAGE_ID {
                return Ok(None);
            }

            path.push((guard, arc, version));
            current_id = child_id;
        }
    }

    fn link_siblings_after_split_locked(
        &self,
        left_id: PageId,
        left_page: &crate::page::Page,
        right_id: PageId,
        right_page: &crate::page::Page,
    ) -> Result<()> {
        let old_next = left_page.next_page_id()?;
        right_page.set_next_page_id(old_next);
        right_page.set_prev_page_id(left_id);
        left_page.set_next_page_id(right_id);

        if old_next != NULL_PAGE_ID {
            // The old next page may be briefly locked by another SMO.  Spin
            // rather than fail: we already hold the left/right latches and the
            // contending thread cannot need either of them.
            loop {
                let next_guard = self.pool.fetch_or_read(old_next)?;
                let next_arc = next_guard.page_arc();
                if let Some(next_write) = next_arc.try_write() {
                    next_write.page().set_prev_page_id(right_id);
                    self.pool.mark_dirty(old_next)?;
                    break;
                }
                std::thread::yield_now();
            }
        }
        Ok(())
    }

    /// Delete `key`. Returns true if the key existed and was removed.
    pub fn delete(&self, key: &[u8]) -> Result<bool> {
        loop {
            match self.lock_coupled_delete(key)? {
                Some(existed) => {
                    self.sync()?;
                    return Ok(existed);
                }
                None => continue,
            }
        }
    }

    // ------------------------------------------------------------------
    // Multi-record transactions (Phase 6)
    // ------------------------------------------------------------------

    /// Begin a new multi-record transaction.
    pub fn begin_txn(&self, isolation: IsolationLevel) -> Result<Transaction> {
        let (txn_id, read_ts) = self.txn_table.begin(isolation)?;
        let txn = Transaction::new(txn_id, read_ts, isolation);
        let record = Record {
            header: RecordHeader::new(RecordType::Begin, txn_id, NULL_LSN, NULL_PAGE_ID, NULL_LSN),
            payload: RecordPayload::Begin,
        };
        // Buffered append: the whole transaction (Begin + ops + Commit/Abort) is
        // synced together at commit/rollback time.
        let lsn = match self.wal() {
            Some(wal) => wal.append_buffered(record)?,
            None => NULL_LSN,
        };
        txn.set_last_lsn(lsn);
        Ok(txn)
    }

    /// Commit `txn`.  Returns the assigned commit timestamp.
    pub fn commit_txn(&self, txn: &Transaction) -> Result<Timestamp> {
        let commit_ts = self.txn_table.reserve_commit_ts();
        // Make every update record durable before writing the commit marker.
        // If this sync fails, no commit record exists, so recovery will undo
        // the transaction and the caller can treat the commit as failed.
        self.sync()?;
        let record = Record {
            header: RecordHeader::new(
                RecordType::Commit,
                txn.txn_id,
                txn.last_lsn(),
                NULL_PAGE_ID,
                NULL_LSN,
            ),
            payload: RecordPayload::Commit { commit_ts },
        };
        if let Some(wal) = self.wal() {
            wal.append_buffered(record)?;
        }
        // Make the commit marker itself durable.
        if let Err(e) = self.sync() {
            // The commit record may have been written to the OS page cache but
            // not fsynced. Truncate the WAL to the last durable length so the
            // uncommitted commit marker is not made durable by a later sync.
            if let Some(wal) = self.wal() {
                let _ = wal.crash();
            }
            return Err(e);
        }
        self.txn_table.recover_committed(txn.txn_id, commit_ts)?;
        Ok(commit_ts)
    }

    /// Roll back `txn`, restoring all values it modified.
    pub fn rollback_txn(&self, txn: &Transaction) -> Result<()> {
        // Undo first so CLRs are durably recorded before the Abort marker.
        // A crash after Abort but before CLRs would otherwise make recovery
        // think the transaction is complete and skip undo.
        let last_clr_lsn = self.undo_transaction(txn.txn_id, txn.last_lsn())?;

        let abort_record = Record {
            header: RecordHeader::new(
                RecordType::Abort,
                txn.txn_id,
                last_clr_lsn,
                NULL_PAGE_ID,
                NULL_LSN,
            ),
            payload: RecordPayload::Abort,
        };
        if let Some(wal) = self.wal() {
            wal.append_buffered(abort_record)?;
        }
        // Make the whole rollback (CLRs + Abort) durable with a single fsync.
        self.sync()?;
        self.txn_table.abort(txn.txn_id)?;
        Ok(())
    }

    /// Undo all records for `txn_id` starting from `last_lsn` and walking
    /// backward via `prev_lsn`.  Appends a CLR for each undone record and
    /// returns the LSN of the last CLR written (or `NULL_LSN` if none).
    fn undo_transaction(&self, txn_id: TxnId, last_lsn: Lsn) -> Result<Lsn> {
        let mut lsn = last_lsn;
        let mut last_clr = NULL_LSN;
        while lsn != NULL_LSN {
            let record = match self.wal() {
                Some(wal) => wal.read_at(lsn)?,
                None => break,
            };
            match &record.payload {
                RecordPayload::Clr { undo_next_lsn, .. } => {
                    lsn = *undo_next_lsn;
                    continue;
                }
                RecordPayload::Begin => break,
                RecordPayload::UpdateCell {
                    cell,
                    old_cell: Some(old_cell),
                    old_header: Some(old_header),
                    ..
                } => {
                    let page_id = record.header.page_id;
                    let guard = self.pool.fetch_or_read(page_id)?;
                    let page = guard.page();
                    let write = Self::acquire_undo_write_latch(page);
                    let page = write.page();
                    if let Some(current) = page.get(&cell.key)? {
                        self.release_value(&current.value);
                    }
                    let image = crate::undo::make_undo_image(old_cell.clone(), *old_header);
                    crate::undo::apply_undo_to_page(page, &old_cell.key, &image)?;
                    self.add_value_ref(&old_cell.value);
                    guard.mark_dirty();
                }
                RecordPayload::DeleteCell {
                    key,
                    old_cell: Some(old_cell),
                    old_header: Some(old_header),
                } => {
                    let page_id = record.header.page_id;
                    let guard = self.pool.fetch_or_read(page_id)?;
                    let page = guard.page();
                    let write = Self::acquire_undo_write_latch(page);
                    let page = write.page();
                    let image = crate::undo::make_undo_image(old_cell.clone(), *old_header);
                    crate::undo::apply_undo_to_page(page, key, &image)?;
                    self.add_value_ref(&old_cell.value);
                    guard.mark_dirty();
                }
                RecordPayload::InsertCell { cell } => {
                    let page_id = record.header.page_id;
                    let guard = self.pool.fetch_or_read(page_id)?;
                    let page = guard.page();
                    let write = Self::acquire_undo_write_latch(page);
                    let page = write.page();
                    if let Some(current) = page.get(&cell.key)? {
                        self.release_value(&current.value);
                    }
                    page.delete(&cell.key)?;
                    guard.mark_dirty();
                }
                _ => {}
            }

            if let Some(wal) = self.wal() {
                last_clr =
                    crate::undo::append_clr(wal, txn_id, lsn, record.header.prev_lsn, &record)?;
            }
            lsn = record.header.prev_lsn;
        }
        Ok(last_clr)
    }

    /// Acquire an exclusive OLC write latch on `page` for undo processing.
    ///
    /// Undo must observe and mutate a stable page image, so it cannot use the
    /// optimistic read path.  The latch is held briefly, so we spin until it is
    /// available; giving up would leave the transaction half-undone.
    fn acquire_undo_write_latch(page: &crate::page::Page) -> WriteGuard<'_> {
        loop {
            if let Some(write) = page.try_write() {
                return write;
            }
            std::thread::yield_now();
        }
    }

    /// Look up `key` within the snapshot of `txn`.
    pub fn get_txn(&self, txn: &Transaction, key: &[u8]) -> Result<Option<Vec<u8>>> {
        loop {
            match self.try_get_txn(txn, key)? {
                Some(value) => return Ok(value),
                None => continue,
            }
        }
    }

    fn try_get_txn(&self, txn: &Transaction, key: &[u8]) -> Result<Option<Option<Vec<u8>>>> {
        let leaf = match self.optimistic_leaf(key)? {
            Some(l) => l,
            None => return Ok(None),
        };

        if self.load_root() != leaf.root_id {
            return Ok(None);
        }

        let page = leaf.guard.page();
        let opt = match page.optimistic() {
            Some(o) => o,
            None => return Ok(None),
        };

        let result = opt.read(|p| -> Result<Option<Vec<u8>>> {
            let opt_cell = match p.get(key)? {
                Some(c) => c,
                None => return Ok(None),
            };
            let header = opt_cell.mvcc.unwrap_or(MvccHeader::autocommit());
            let visible = resolve_version_chain(
                &header,
                &opt_cell.value.as_value_kind(),
                txn.read_ts,
                txn.txn_id,
                self.txn_table.as_ref(),
                |lsn| self.fetch_wal_version(lsn),
            )?;
            Ok(match visible {
                VisibleValue::Found(OwnedValue::Inline(v)) => Some(v),
                VisibleValue::Found(OwnedValue::ValueLog { offset, len }) => {
                    let bytes = self
                        .value_log()
                        .expect("value log missing")
                        .read(offset, len)?;
                    Some(bytes)
                }
                VisibleValue::Found(OwnedValue::Tombstone) => None,
                VisibleValue::NotFound | VisibleValue::InWal(_) => None,
            })
        });

        match result {
            None => Ok(None),
            Some(Err(e)) => Err(e),
            Some(Ok(value)) => {
                if !leaf.path_valid(key) {
                    return Ok(None);
                }
                Ok(Some(value))
            }
        }
    }

    fn fetch_wal_version(&self, lsn: Lsn) -> Result<Option<(MvccHeader, OwnedValue)>> {
        let record = match self.wal() {
            Some(wal) => wal.read_at(lsn)?,
            None => return Ok(None),
        };
        match record.payload {
            RecordPayload::UpdateCell {
                old_cell: Some(old_cell),
                old_header: Some(old_header),
                ..
            }
            | RecordPayload::DeleteCell {
                old_cell: Some(old_cell),
                old_header: Some(old_header),
                ..
            } => Ok(Some((old_header, old_cell.value))),
            RecordPayload::InsertCell { cell } => {
                let header = cell.mvcc.unwrap_or(MvccHeader::autocommit());
                Ok(Some((header, cell.value)))
            }
            _ => Ok(None),
        }
    }

    /// Resolve the visible value for `cell` from the point of view of a reader
    /// with `read_ts` and `self_txn_id`.
    pub(crate) fn resolve_cell_value(
        &self,
        cell: &OwnedCell,
        read_ts: Timestamp,
        self_txn_id: TxnId,
    ) -> Result<Option<Vec<u8>>> {
        let header = cell.mvcc.unwrap_or(MvccHeader::autocommit());
        let visible = resolve_version_chain(
            &header,
            &cell.value.as_value_kind(),
            read_ts,
            self_txn_id,
            self.txn_table.as_ref(),
            |lsn| self.fetch_wal_version(lsn),
        )?;
        Ok(match visible {
            VisibleValue::Found(OwnedValue::Inline(v)) => Some(v),
            VisibleValue::Found(OwnedValue::ValueLog { offset, len }) => {
                let bytes = self
                    .value_log()
                    .ok_or(Error::Unsupported("value log missing"))?
                    .read(offset, len)?;
                Some(bytes)
            }
            VisibleValue::Found(OwnedValue::Tombstone) => None,
            VisibleValue::NotFound | VisibleValue::InWal(_) => None,
        })
    }

    /// Insert or replace `value` for `key` on behalf of `txn`.
    pub fn insert_txn(&self, txn: &Transaction, key: &[u8], value: &[u8]) -> Result<()> {
        let value_kind = self.prepare_value(value)?;
        loop {
            match self.lock_coupled_insert_txn(txn, key, &value_kind)? {
                Some(old_value) => {
                    if let Some(old) = old_value {
                        self.release_value(&old);
                    }
                    return Ok(());
                }
                None => continue,
            }
        }
    }

    fn lock_coupled_insert_txn(
        &self,
        txn: &Transaction,
        key: &[u8],
        value: &ValueKind<'_>,
    ) -> Result<Option<Option<OwnedValue>>> {
        let target = match self.optimistic_path_to_leaf(key)? {
            Some(t) => t,
            None => return Ok(None),
        };

        let leaf_arc = Arc::clone(&target.leaf_arc);
        let leaf_write = match leaf_arc.try_write() {
            Some(w) => w,
            None => return Ok(None),
        };

        if self.load_root() != target.root_id {
            return Ok(None);
        }
        if !target.path_valid(key) {
            return Ok(None);
        }

        let old_cell = leaf_write.page().get(key)?;
        match self.write_txn_cell(leaf_write.page(), txn, key, value) {
            Ok(lsn) => {
                txn.set_last_lsn(lsn);
                self.pool.mark_dirty(leaf_arc.id)?;
                return Ok(Some(old_cell.map(|c| c.value)));
            }
            Err(Error::PageFull) => {}
            Err(e) => return Err(e),
        }

        // Root-leaf split: the leaf is the only page in the tree.
        if target.path.is_empty() {
            return self.split_root_leaf_txn_locked(leaf_arc.id, leaf_write, txn, key, value);
        }

        // The leaf is full and has a parent.  Lock the whole path, split the
        // leaf (structure modifications are autocommit), then insert the
        // transactional cell into the correct half.
        let path = target.path;
        let mut locked_arcs: Vec<Arc<crate::page::Page>> = Vec::with_capacity(path.len() + 1);
        let mut ancestor_versions: Vec<u64> = Vec::with_capacity(path.len());
        locked_arcs.push(Arc::clone(&leaf_arc));
        for (_, arc, version) in path.iter().rev() {
            locked_arcs.push(Arc::clone(arc));
            ancestor_versions.push(*version);
        }

        let mut locked: Vec<WriteGuard<'_>> = Vec::with_capacity(locked_arcs.len());
        locked.push(leaf_write);
        for i in 1..locked_arcs.len() {
            let write = match locked_arcs[i].try_write() {
                Some(w) => w,
                None => return Ok(None),
            };
            if locked_arcs[i].latch_word() & !1 != ancestor_versions[i - 1] {
                return Ok(None);
            }
            locked.push(write);
        }

        let right_guard = self.pool.new_page()?;
        let right_arc = right_guard.page_arc();
        let right_write = match right_arc.try_write() {
            Some(w) => w,
            None => return Ok(None),
        };
        right_write.page().set_leaf();

        let leaf_id = locked_arcs[0].id;
        let separator = split_leaf(locked[0].page(), right_write.page())?;
        self.log_split(locked[0].page(), right_write.page(), &separator, false)?;
        let old_cell = locked[0].page().get(key)?;
        let lsn = if key >= separator.as_slice() {
            self.write_txn_cell(right_write.page(), txn, key, value)?
        } else {
            self.write_txn_cell(locked[0].page(), txn, key, value)?
        };
        txn.set_last_lsn(lsn);
        self.pool.mark_dirty(leaf_id)?;
        self.pool.mark_dirty(right_arc.id)?;

        self.link_siblings_after_split_locked(
            leaf_id,
            locked[0].page(),
            right_arc.id,
            right_write.page(),
        )?;

        let mut split = SplitResult {
            separator,
            right_page_id: right_arc.id,
        };

        for parent_write in locked.iter().take(locked.len().saturating_sub(1)).skip(1) {
            let parent_id = parent_write.page().id;
            let child_bytes = encode_page_id(split.right_page_id);

            match page_insert_logged(
                parent_write.page(),
                self.wal(),
                &split.separator,
                &ValueKind::Inline(&child_bytes),
            ) {
                Ok(_) => {
                    self.pool.mark_dirty(parent_id)?;
                    return Ok(Some(None));
                }
                Err(Error::PageFull) => {
                    let new_right_guard = self.pool.new_page()?;
                    let new_right_arc = new_right_guard.page_arc();
                    let new_right_write = match new_right_arc.try_write() {
                        Some(w) => w,
                        None => return Ok(None),
                    };
                    new_right_write.page().set_internal();

                    let promoted = split_internal(parent_write.page(), new_right_write.page())?;
                    self.log_split(parent_write.page(), new_right_write.page(), &promoted, true)?;
                    if split.separator.as_slice() > promoted.as_slice() {
                        page_insert_logged(
                            new_right_write.page(),
                            self.wal(),
                            &split.separator,
                            &ValueKind::Inline(&child_bytes),
                        )?;
                    } else {
                        page_insert_logged(
                            parent_write.page(),
                            self.wal(),
                            &split.separator,
                            &ValueKind::Inline(&child_bytes),
                        )?;
                    }
                    self.pool.mark_dirty(parent_id)?;
                    self.pool.mark_dirty(new_right_arc.id)?;

                    split = SplitResult {
                        separator: promoted,
                        right_page_id: new_right_arc.id,
                    };
                }
                Err(e) => return Err(e),
            }
        }

        let root_write = locked
            .last()
            .expect("locked path contains at least the leaf");
        let root_id = root_write.page().id;
        let child_bytes = encode_page_id(split.right_page_id);

        match page_insert_logged(
            root_write.page(),
            self.wal(),
            &split.separator,
            &ValueKind::Inline(&child_bytes),
        ) {
            Ok(_) => {
                self.pool.mark_dirty(root_id)?;
                return Ok(Some(None));
            }
            Err(Error::PageFull) => {}
            Err(e) => return Err(e),
        }

        let new_right_guard = self.pool.new_page()?;
        let new_right_arc = new_right_guard.page_arc();
        let new_right_write = match new_right_arc.try_write() {
            Some(w) => w,
            None => return Ok(None),
        };
        new_right_write.page().set_internal();

        let promoted = split_internal(root_write.page(), new_right_write.page())?;
        self.log_split(root_write.page(), new_right_write.page(), &promoted, true)?;
        if split.separator.as_slice() > promoted.as_slice() {
            page_insert_logged(
                new_right_write.page(),
                self.wal(),
                &split.separator,
                &ValueKind::Inline(&child_bytes),
            )?;
        } else {
            page_insert_logged(
                root_write.page(),
                self.wal(),
                &split.separator,
                &ValueKind::Inline(&child_bytes),
            )?;
        }
        self.pool.mark_dirty(root_id)?;
        self.pool.mark_dirty(new_right_arc.id)?;

        let new_root_guard = self.pool.new_page()?;
        let new_root_arc = new_root_guard.page_arc();
        let new_root_write = match new_root_arc.try_write() {
            Some(w) => w,
            None => return Ok(None),
        };
        new_root_write.page().set_internal();
        self.log_new_root(new_root_write.page(), root_id, &promoted, new_right_arc.id)?;
        new_root_write.page().set_leftmost_child(root_id);
        let bytes = encode_page_id(new_right_arc.id);
        new_root_write
            .page()
            .insert(&promoted, &ValueKind::Inline(&bytes))?;
        self.pool.mark_dirty(new_root_arc.id)?;

        self.log_set_root(new_root_arc.id)?;
        self.store_root(new_root_arc.id);
        Ok(Some(old_cell.map(|c| c.value)))
    }

    /// Write a transactional cell into `page`.  If the key already exists this
    /// produces an `UpdateCell` record; otherwise an `InsertCell` record.
    fn write_txn_cell(
        &self,
        page: &crate::page::Page,
        txn: &Transaction,
        key: &[u8],
        value: &ValueKind<'_>,
    ) -> Result<Lsn> {
        let existing = page.get(key)?;
        if let Some(ref cell) = existing {
            self.check_write_conflict(txn, cell)?;
        }

        let prev_lsn = txn.last_lsn();
        let new_mvcc = MvccHeader {
            begin_ts: txn.txn_id,
            end_ts: NULL_TXN_ID,
            prev_version_lsn: NULL_LSN,
        };

        if let Some(old_cell) = existing {
            let old_header = old_cell.mvcc.unwrap_or(MvccHeader::autocommit());
            page_update_txn_logged(
                page,
                self.wal(),
                txn.txn_id,
                prev_lsn,
                key,
                value,
                new_mvcc,
                old_cell,
                old_header,
            )
        } else {
            page_insert_txn_logged(page, self.wal(), txn.txn_id, prev_lsn, key, value, new_mvcc)
        }
    }

    fn split_root_leaf_txn_locked(
        &self,
        root_id: PageId,
        root_write: WriteGuard<'_>,
        txn: &Transaction,
        key: &[u8],
        value: &ValueKind<'_>,
    ) -> Result<Option<Option<OwnedValue>>> {
        let right_guard = self.pool.new_page()?;
        let right_arc = right_guard.page_arc();
        let right_write = match right_arc.try_write() {
            Some(w) => w,
            None => return Ok(None),
        };
        right_write.page().set_leaf();

        let separator = split_leaf(root_write.page(), right_write.page())?;
        self.log_split(root_write.page(), right_write.page(), &separator, false)?;
        let old_cell = root_write.page().get(key)?;
        let lsn = if key >= separator.as_slice() {
            self.write_txn_cell(right_write.page(), txn, key, value)?
        } else {
            self.write_txn_cell(root_write.page(), txn, key, value)?
        };
        txn.set_last_lsn(lsn);
        self.pool.mark_dirty(root_id)?;
        self.pool.mark_dirty(right_arc.id)?;

        self.link_siblings_after_split_locked(
            root_id,
            root_write.page(),
            right_arc.id,
            right_write.page(),
        )?;

        let new_root_guard = self.pool.new_page()?;
        let new_root_arc = new_root_guard.page_arc();
        let new_root_write = match new_root_arc.try_write() {
            Some(w) => w,
            None => return Ok(None),
        };
        new_root_write.page().set_internal();
        self.log_new_root(new_root_write.page(), root_id, &separator, right_arc.id)?;
        new_root_write.page().set_leftmost_child(root_id);
        let bytes = encode_page_id(right_arc.id);
        new_root_write
            .page()
            .insert(&separator, &ValueKind::Inline(&bytes))?;
        self.pool.mark_dirty(new_root_arc.id)?;

        self.log_set_root(new_root_arc.id)?;
        self.store_root(new_root_arc.id);
        Ok(Some(old_cell.map(|c| c.value)))
    }

    /// Delete `key` on behalf of `txn`.  Returns true if the key existed.
    pub fn delete_txn(&self, txn: &Transaction, key: &[u8]) -> Result<bool> {
        loop {
            match self.lock_coupled_delete_txn(txn, key)? {
                Some(existed) => return Ok(existed),
                None => continue,
            }
        }
    }

    fn lock_coupled_delete_txn(&self, txn: &Transaction, key: &[u8]) -> Result<Option<bool>> {
        let target = match self.optimistic_path_to_leaf(key)? {
            Some(t) => t,
            None => return Ok(None),
        };

        let leaf_arc = Arc::clone(&target.leaf_arc);
        let leaf_write = match leaf_arc.try_write() {
            Some(w) => w,
            None => return Ok(None),
        };

        if self.load_root() != target.root_id {
            return Ok(None);
        }
        if !target.path_valid(key) {
            return Ok(None);
        }

        let old_cell = match leaf_write.page().get(key)? {
            Some(c) => c,
            None => return Ok(Some(false)),
        };

        self.check_write_conflict(txn, &old_cell)?;

        let old_header = old_cell.mvcc.unwrap_or(MvccHeader::autocommit());
        let released_value = old_cell.value.clone();
        let lsn = page_delete_txn_logged(
            leaf_write.page(),
            self.wal(),
            txn.txn_id,
            txn.last_lsn(),
            key,
            old_cell,
            old_header,
        )?;
        self.release_value(&released_value);
        txn.set_last_lsn(lsn);
        self.pool.mark_dirty(leaf_arc.id)?;
        Ok(Some(true))
    }

    /// Enforce first-writer-wins for Snapshot isolation.
    fn check_write_conflict(&self, txn: &Transaction, cell: &OwnedCell) -> Result<()> {
        if txn.isolation != IsolationLevel::Snapshot {
            return Ok(());
        }
        let header = cell.mvcc.unwrap_or(MvccHeader::autocommit());
        if header.begin_ts == NULL_TXN_ID || header.begin_ts == txn.txn_id {
            return Ok(());
        }
        if let Some(commit_ts) = self.txn_table.commit_ts(header.begin_ts)
            && commit_ts > txn.read_ts
        {
            return Err(Error::Conflict);
        }
        Ok(())
    }

    fn lock_coupled_delete(&self, key: &[u8]) -> Result<Option<bool>> {
        let target = match self.optimistic_path_to_leaf(key)? {
            Some(t) => t,
            None => return Ok(None),
        };

        let leaf_arc = Arc::clone(&target.leaf_arc);
        let leaf_write = match leaf_arc.try_write() {
            Some(w) => w,
            None => return Ok(None),
        };

        // A root split may have changed the root pointer since we started the
        // traversal.  If so, the captured path is stale and we must retry.
        if self.load_root() != target.root_id {
            return Ok(None);
        }
        // Validate the captured root-to-leaf path.
        if !target.path_valid(key) {
            return Ok(None);
        }

        let leaf_id = leaf_arc.id;
        let will_underflow =
            leaf_write.page().live_count()? <= self.min_cells && leaf_id != self.root_page_id();

        if will_underflow && !target.path.is_empty() {
            // Deleting this key will make the leaf underflow.  Lock the whole
            // root-to-leaf path *before* mutating anything so that, once we
            // delete, we are guaranteed to complete redistribution/merge without
            // needing to undo the deletion.
            match self.lock_path_and_handle_leaf_underflow(target, leaf_write, key)? {
                None => Ok(None),
                Some(dead) => {
                    for id in dead {
                        self.retire_page(id)?;
                    }
                    Ok(Some(true))
                }
            }
        } else {
            // Common case: deletion does not cause underflow (or the leaf is the
            // root and is allowed to shrink).
            let old_cell = leaf_write.page().get(key)?;
            let existed = page_delete_logged(leaf_write.page(), self.wal(), key)?;
            if !existed {
                return Ok(Some(false));
            }
            if let Some(cell) = old_cell {
                self.release_value(&cell.value);
            }
            self.pool.mark_dirty(leaf_id)?;
            Ok(Some(true))
        }
    }

    /// Lock the entire root-to-leaf path, delete `key` from the leaf, and handle
    /// the resulting underflow.  Returns `None` if any latch could not be
    /// acquired or a version changed (caller retries from the root).  Dead page
    /// ids are returned in the `Some` case and must be freed by the caller.
    fn lock_path_and_handle_leaf_underflow(
        &self,
        target: OptimisticPath,
        leaf_write: WriteGuard<'_>,
        key: &[u8],
    ) -> Result<Option<Vec<PageId>>> {
        let mut locked_arcs: Vec<Arc<crate::page::Page>> =
            Vec::with_capacity(target.path.len() + 1);
        let mut ancestor_versions: Vec<u64> = Vec::with_capacity(target.path.len());
        locked_arcs.push(Arc::clone(&target.leaf_arc));
        for (_, arc, version) in target.path.iter().rev() {
            locked_arcs.push(Arc::clone(arc));
            ancestor_versions.push(*version);
        }

        let mut locked: Vec<WriteGuard<'_>> = Vec::with_capacity(locked_arcs.len());
        locked.push(leaf_write);
        for i in 1..locked_arcs.len() {
            let write = match locked_arcs[i].try_write() {
                Some(w) => w,
                None => return Ok(None),
            };
            if locked_arcs[i].latch_word() & !1 != ancestor_versions[i - 1] {
                return Ok(None);
            }
            locked.push(write);
        }

        let leaf_id = locked[0].page().id;
        let old_cell = locked[0].page().get(key)?;
        let existed = page_delete_logged(locked[0].page(), self.wal(), key)?;
        if existed {
            if let Some(cell) = old_cell {
                self.release_value(&cell.value);
            }
            self.pool.mark_dirty(leaf_id)?;
        }

        // Whether we deleted the key now or on a previous attempt that failed to
        // acquire a sibling latch, if the leaf has underflowed we must resolve it
        // while we still hold the root-to-leaf path.
        let live = locked[0].page().live_count()?;
        if live >= self.min_cells || leaf_id == self.root_page_id() {
            return Ok(Some(Vec::new()));
        }

        let mut dead: Vec<PageId> = Vec::new();
        self.handle_leaf_underflow_locked(&mut locked, &mut dead)?;
        Ok(Some(dead))
    }

    // ------------------------------------------------------------------
    // OLC underflow handling
    // ------------------------------------------------------------------

    /// Handle a leaf underflow while holding a write latch on every page from
    /// the leaf up to the root.  `locked[0]` is the leaf, `locked[1]` its parent,
    /// and so on.  Sibling pages are locked on demand.  Dead page ids are pushed
    /// to `dead`; the caller frees them after releasing all latches.
    fn handle_leaf_underflow_locked(
        &self,
        locked: &mut Vec<WriteGuard<'_>>,
        dead: &mut Vec<PageId>,
    ) -> Result<Option<()>> {
        let leaf_id = locked[0].page().id;
        let pos = child_position(locked[1].page(), leaf_id)?
            .ok_or_else(|| Error::Corruption("underflowed leaf not found in parent".into()))?;
        let right_sibling = right_child_at(locked[1].page(), pos)?;
        let left_sibling = if pos > 0 {
            Some(child_at(locked[1].page(), pos - 1)?)
        } else {
            None
        };

        // Redistribute from right sibling.
        if let Some(right_id) = right_sibling
            && self
                .try_redistribute_leaf(locked, leaf_id, right_id, true)?
                .is_some()
        {
            return Ok(Some(()));
        }

        // Redistribute from left sibling.
        if let Some(left_id) = left_sibling
            && self
                .try_redistribute_leaf(locked, leaf_id, left_id, false)?
                .is_some()
        {
            return Ok(Some(()));
        }

        // Merge with a sibling.
        let merged = if let Some(right_id) = right_sibling {
            self.merge_leaf_with_sibling(locked, leaf_id, right_id, true, dead)?
        } else if let Some(left_id) = left_sibling {
            self.merge_leaf_with_sibling(locked, leaf_id, left_id, false, dead)?
        } else {
            return Err(Error::Corruption(format!(
                "leaf page {leaf_id} has no sibling to merge with"
            )));
        };
        if merged.is_none() {
            return Ok(None);
        }

        // The leaf level is resolved; move up and propagate any internal
        // underflow.  Pop the leaf so `locked[0]` becomes the parent.
        locked.remove(0);
        self.propagate_internal_underflow_locked(locked, dead)
    }

    /// Try to move one cell from `sibling_id` into the underflowed leaf.  On
    /// success all modified pages are marked dirty and `Some(())` is returned.
    /// On latch contention `None` is returned so the caller can retry from the
    /// root.  `sibling_is_right` distinguishes the right sibling (true) from the
    /// left sibling (false).
    fn try_redistribute_leaf(
        &self,
        locked: &mut [WriteGuard<'_>],
        leaf_id: PageId,
        sibling_id: PageId,
        sibling_is_right: bool,
    ) -> Result<Option<()>> {
        let sibling_guard = self.pool.fetch_or_read(sibling_id)?;
        let sibling_arc = sibling_guard.page_arc();
        let sibling_write = match sibling_arc.try_write() {
            Some(w) => w,
            None => return Ok(None),
        };
        if !can_give_cell(sibling_write.page(), self.min_cells) {
            return Ok(None);
        }

        let leaf_write = &locked[0];
        let parent_write = &locked[1];
        let parent_id = parent_write.page().id;

        if sibling_is_right {
            let cell = sibling_write.page().get_by_slot(0)?;
            page_move_cell_logged(sibling_write.page(), leaf_write.page(), self.wal(), &cell)?;
            self.pool.mark_dirty(leaf_id)?;
            self.pool.mark_dirty(sibling_id)?;
            let new_sep = sibling_write.page().get_by_slot(0)?.key;
            let pos = child_position(parent_write.page(), leaf_id)?
                .ok_or_else(|| Error::Corruption("underflowed leaf not found in parent".into()))?;
            let old_sep = parent_write.page().get_by_slot(pos)?.key;
            let child_bytes = encode_page_id(sibling_id);
            page_replace_key_logged(
                parent_write.page(),
                self.wal(),
                &old_sep,
                &new_sep,
                &ValueKind::Inline(&child_bytes),
            )?;
        } else {
            let last_idx = sibling_write.page().slot_count()? - 1;
            let cell = sibling_write.page().get_by_slot(last_idx)?;
            page_move_cell_logged(sibling_write.page(), leaf_write.page(), self.wal(), &cell)?;
            self.pool.mark_dirty(leaf_id)?;
            self.pool.mark_dirty(sibling_id)?;
            let new_sep = leaf_write.page().get_by_slot(0)?.key;
            let pos = child_position(parent_write.page(), leaf_id)?
                .ok_or_else(|| Error::Corruption("underflowed leaf not found in parent".into()))?;
            let old_sep = parent_write.page().get_by_slot(pos - 1)?.key;
            let child_bytes = encode_page_id(leaf_id);
            page_replace_key_logged(
                parent_write.page(),
                self.wal(),
                &old_sep,
                &new_sep,
                &ValueKind::Inline(&child_bytes),
            )?;
        }
        self.pool.mark_dirty(parent_id)?;

        // `sibling_guard` is dropped on return; the caller does not continue with
        // a merge, so the sibling remains reachable and does not need freeing.
        Ok(Some(()))
    }

    /// Merge `sibling_id` with the leaf and remove the separating entry from the
    /// parent.  If `sibling_is_right` the right sibling is merged into the leaf;
    /// otherwise the leaf is merged into the left sibling.  The dead page id is
    /// pushed to `dead` for later freeing.
    fn merge_leaf_with_sibling(
        &self,
        locked: &mut [WriteGuard<'_>],
        leaf_id: PageId,
        sibling_id: PageId,
        sibling_is_right: bool,
        dead: &mut Vec<PageId>,
    ) -> Result<Option<()>> {
        let sibling_guard = self.pool.fetch_or_read(sibling_id)?;
        let sibling_arc = sibling_guard.page_arc();
        let sibling_write = match sibling_arc.try_write() {
            Some(w) => w,
            None => {
                // To avoid symmetric livelock when two underflowed adjacent
                // leaves each try to merge into the other, the thread holding
                // the leaf with the smaller page id spins briefly; the other
                // releases its latches and retries from the root.
                if leaf_id < sibling_id {
                    let mut spins = 0;
                    loop {
                        if let Some(w) = sibling_arc.try_write() {
                            break w;
                        }
                        spins += 1;
                        if spins > 1000 {
                            return Ok(None);
                        }
                        std::thread::yield_now();
                    }
                } else {
                    return Ok(None);
                }
            }
        };

        let parent_write = &locked[1];
        let parent_id = parent_write.page().id;
        let pos = child_position(parent_write.page(), leaf_id)?
            .ok_or_else(|| Error::Corruption("underflowed leaf not found in parent".into()))?;

        if sibling_is_right {
            self.log_merge(locked[0].page(), sibling_id, false, &[], NULL_PAGE_ID)?;
            merge_leaf_right_into_left(locked[0].page(), sibling_write.page(), &self.pool)?;
            self.pool.mark_dirty(leaf_id)?;
            let separator = parent_write.page().get_by_slot(pos)?.key;
            page_delete_logged(parent_write.page(), self.wal(), &separator)?;
            dead.push(sibling_id);
        } else {
            self.log_merge(sibling_write.page(), leaf_id, false, &[], NULL_PAGE_ID)?;
            merge_leaf_right_into_left(sibling_write.page(), locked[0].page(), &self.pool)?;
            self.pool.mark_dirty(sibling_id)?;
            let separator = parent_write.page().get_by_slot(pos - 1)?.key;
            page_delete_logged(parent_write.page(), self.wal(), &separator)?;
            dead.push(leaf_id);
        }
        self.pool.mark_dirty(parent_id)?;

        // `sibling_guard` is dropped here; the dead page is freed by the caller
        // after all tree latches are released.
        Ok(Some(()))
    }

    /// Propagate an internal-node underflow upward.  `locked[0]` is the node that
    /// may have underflowed; `locked[1]` is its parent.  Dead page ids are pushed
    /// to `dead` and freed by the caller after all latches are released.
    fn propagate_internal_underflow_locked(
        &self,
        locked: &mut Vec<WriteGuard<'_>>,
        dead: &mut Vec<PageId>,
    ) -> Result<Option<()>> {
        if locked.is_empty() {
            return Ok(Some(()));
        }
        let node_id = locked[0].page().id;
        let live = locked[0].page().live_count()?;

        let root_id = self.root_page_id();
        if node_id == root_id {
            if locked[0].page().is_internal() && live == 0 {
                let new_root_id = locked[0].page().leftmost_child()?;
                if new_root_id != NULL_PAGE_ID {
                    // The old root page becomes unreachable; record it as dead
                    // and install the new root.  Poison the old root (clear its
                    // leftmost child) so that any in-flight traversal that
                    // captured the old root id and version fails validation
                    // before it can mutate the now-dead page.
                    self.log_set_root(new_root_id)?;
                    self.log_move_rightmost(locked[0].page(), new_root_id, NULL_PAGE_ID)?;
                    self.store_root(new_root_id);
                    locked[0].page().set_leftmost_child(NULL_PAGE_ID);
                    dead.push(node_id);
                }
            }
            return Ok(Some(()));
        }

        if live >= self.min_cells {
            return Ok(Some(()));
        }

        let pos = child_position(locked[1].page(), node_id)?.ok_or_else(|| {
            Error::Corruption("underflowed internal node not found in parent".into())
        })?;
        let right_sibling = right_child_at(locked[1].page(), pos)?;
        let left_sibling = if pos > 0 {
            Some(child_at(locked[1].page(), pos - 1)?)
        } else {
            None
        };

        // Redistribute from right sibling.
        if let Some(right_id) = right_sibling
            && self
                .try_redistribute_internal(locked, node_id, right_id, true)?
                .is_some()
        {
            return Ok(Some(()));
        }

        // Redistribute from left sibling.
        if let Some(left_id) = left_sibling
            && self
                .try_redistribute_internal(locked, node_id, left_id, false)?
                .is_some()
        {
            return Ok(Some(()));
        }

        // Merge with a sibling.
        let merged = if let Some(right_id) = right_sibling {
            self.merge_internal_with_sibling(locked, node_id, right_id, true, dead)?
        } else if let Some(left_id) = left_sibling {
            self.merge_internal_with_sibling(locked, node_id, left_id, false, dead)?
        } else {
            return Err(Error::Corruption(format!(
                "internal page {node_id} has no sibling to merge with"
            )));
        };
        if merged.is_none() {
            return Ok(None);
        }

        // Move up: pop the resolved node so the parent becomes the underflow
        // candidate.
        locked.remove(0);
        self.propagate_internal_underflow_locked(locked, dead)
    }

    /// Try to move one entry from `sibling_id` into the underflowed internal node.
    fn try_redistribute_internal(
        &self,
        locked: &mut [WriteGuard<'_>],
        node_id: PageId,
        sibling_id: PageId,
        sibling_is_right: bool,
    ) -> Result<Option<()>> {
        let sibling_guard = self.pool.fetch_or_read(sibling_id)?;
        let sibling_arc = sibling_guard.page_arc();
        let sibling_write = match sibling_arc.try_write() {
            Some(w) => w,
            None => return Ok(None),
        };
        if !can_give_cell(sibling_write.page(), self.min_cells) {
            return Ok(None);
        }

        let node_write = &locked[0];
        let parent_write = &locked[1];
        let parent_id = parent_write.page().id;
        let pos = child_position(parent_write.page(), node_id)?.ok_or_else(|| {
            Error::Corruption("underflowed internal node not found in parent".into())
        })?;

        if sibling_is_right {
            // Parent separator becomes a new cell in the node; the right sibling's
            // old leftmost child is the value.  The right sibling's first cell is
            // removed and its child becomes the right sibling's new leftmost.
            let separator = parent_write.page().get_by_slot(pos)?.key;
            let donor_old_leftmost = sibling_write.page().leftmost_child()?;
            let child_bytes = encode_page_id(donor_old_leftmost);
            page_insert_logged(
                node_write.page(),
                self.wal(),
                &separator,
                &ValueKind::Inline(&child_bytes),
            )?;

            let first = sibling_write.page().get_by_slot(0)?;
            let new_separator = first.key.clone();
            let donor_new_leftmost = decode_page_id(&first.value.as_value_kind())?;
            page_delete_logged(sibling_write.page(), self.wal(), &first.key)?;
            page_set_leftmost_child_logged(
                sibling_write.page(),
                self.wal(),
                donor_old_leftmost,
                donor_new_leftmost,
            )?;

            self.pool.mark_dirty(node_id)?;
            self.pool.mark_dirty(sibling_id)?;
            let parent_child_bytes = encode_page_id(sibling_id);
            page_replace_key_logged(
                parent_write.page(),
                self.wal(),
                &separator,
                &new_separator,
                &ValueKind::Inline(&parent_child_bytes),
            )?;
        } else {
            // Parent separator becomes a new cell in the node; the node's old
            // leftmost child is the value.  The left sibling's last cell is removed
            // and its child becomes the node's new leftmost.
            let separator = parent_write.page().get_by_slot(pos - 1)?.key;
            let receiver_old_leftmost = node_write.page().leftmost_child()?;
            let child_bytes = encode_page_id(receiver_old_leftmost);
            page_insert_logged(
                node_write.page(),
                self.wal(),
                &separator,
                &ValueKind::Inline(&child_bytes),
            )?;

            let last_idx = sibling_write.page().slot_count()? - 1;
            let last = sibling_write.page().get_by_slot(last_idx)?;
            let new_separator = last.key.clone();
            let node_new_leftmost = decode_page_id(&last.value.as_value_kind())?;
            page_delete_logged(sibling_write.page(), self.wal(), &last.key)?;
            page_set_leftmost_child_logged(
                node_write.page(),
                self.wal(),
                receiver_old_leftmost,
                node_new_leftmost,
            )?;

            self.pool.mark_dirty(node_id)?;
            self.pool.mark_dirty(sibling_id)?;
            let parent_child_bytes = encode_page_id(node_id);
            page_replace_key_logged(
                parent_write.page(),
                self.wal(),
                &separator,
                &new_separator,
                &ValueKind::Inline(&parent_child_bytes),
            )?;
        }
        self.pool.mark_dirty(parent_id)?;
        Ok(Some(()))
    }

    /// Merge `sibling_id` with the underflowed internal node.
    fn merge_internal_with_sibling(
        &self,
        locked: &mut [WriteGuard<'_>],
        node_id: PageId,
        sibling_id: PageId,
        sibling_is_right: bool,
        dead: &mut Vec<PageId>,
    ) -> Result<Option<()>> {
        let sibling_guard = self.pool.fetch_or_read(sibling_id)?;
        let sibling_arc = sibling_guard.page_arc();
        let sibling_write = match sibling_arc.try_write() {
            Some(w) => w,
            None => {
                // Same priority spin as leaf merges to avoid symmetric livelock.
                if node_id < sibling_id {
                    let mut spins = 0;
                    loop {
                        if let Some(w) = sibling_arc.try_write() {
                            break w;
                        }
                        spins += 1;
                        if spins > 1000 {
                            return Ok(None);
                        }
                        std::thread::yield_now();
                    }
                } else {
                    return Ok(None);
                }
            }
        };

        let parent_write = &locked[1];
        let parent_id = parent_write.page().id;
        let pos = child_position(parent_write.page(), node_id)?.ok_or_else(|| {
            Error::Corruption("underflowed internal node not found in parent".into())
        })?;

        if sibling_is_right {
            let separator = parent_write.page().get_by_slot(pos)?.key;
            let victim_leftmost = sibling_write.page().leftmost_child()?;
            self.log_merge(
                locked[0].page(),
                sibling_id,
                false,
                &separator,
                victim_leftmost,
            )?;
            merge_internal_right_into_left(locked[0].page(), sibling_write.page(), &separator)?;
            self.pool.mark_dirty(node_id)?;
            page_delete_logged(parent_write.page(), self.wal(), &separator)?;
            dead.push(sibling_id);
        } else {
            let separator = parent_write.page().get_by_slot(pos - 1)?.key;
            let victim_leftmost = locked[0].page().leftmost_child()?;
            self.log_merge(
                sibling_write.page(),
                node_id,
                false,
                &separator,
                victim_leftmost,
            )?;
            merge_internal_right_into_left(sibling_write.page(), locked[0].page(), &separator)?;
            self.pool.mark_dirty(sibling_id)?;
            page_delete_logged(parent_write.page(), self.wal(), &separator)?;
            dead.push(node_id);
        }
        self.pool.mark_dirty(parent_id)?;
        Ok(Some(()))
    }

    /// Validate the entire tree structure.
    pub fn check_integrity(&self) -> Result<()> {
        let root_id = self.root_page_id();
        let guard = self.pool.fetch_or_read(root_id)?;
        let root = guard.page();
        if !root.is_leaf() && !root.is_internal() {
            return Err(Error::Corruption(format!(
                "root page {root_id} has neither leaf nor internal flag"
            )));
        }
        if root.is_leaf() && root.leftmost_child()? != NULL_PAGE_ID {
            return Err(Error::Corruption(format!(
                "root leaf page {root_id} has a leftmost child"
            )));
        }
        let mut seen = HashSet::new();
        seen.insert(root_id);
        let mut leaf_prev: Option<PageId> = None;
        if let Err(e) = self.check_node(root, &mut seen, &mut leaf_prev, true, None, None) {
            eprintln!("Integrity check failed: {e}");
            return Err(e);
        }
        Ok(())
    }

    /// Validate the tree structure and every value-log reference.
    ///
    /// This walks all reachable leaf pages, counts value-log references, and
    /// verifies that the counts match the value-log refcount table and that
    /// every referenced value can be read from disk.  It is safe to call while
    /// the tree is quiesced; under concurrent writers the counts may be
    /// transiently inconsistent and the caller should retry.
    pub fn check_integrity_with_value_log(&self) -> Result<()> {
        self.check_integrity()?;

        let value_log = match self.value_log() {
            Some(vl) => vl,
            None => return Ok(()),
        };

        let mut observed: HashMap<(ValueOffset, ValueLen), usize> = HashMap::new();

        // Descend to the leftmost leaf.
        let mut current_id = self.root_page_id();
        loop {
            let guard = self.pool.fetch_or_read(current_id)?;
            let page = guard.page();
            if page.is_leaf() {
                break;
            }
            current_id = page.leftmost_child()?;
        }

        // Walk the leaf sibling chain and collect value-log references.
        while current_id != NULL_PAGE_ID {
            let guard = self.pool.fetch_or_read(current_id)?;
            let page = guard.page();
            for idx in 0..page.slot_count()? {
                if page.read_slot(idx)?.is_deleted() {
                    continue;
                }
                let cell = page.get_by_slot(idx)?;
                if let OwnedValue::ValueLog { offset, len } = cell.value {
                    *observed.entry((offset, len)).or_insert(0) += 1;
                    // Verify the on-disk record is readable and its length
                    // matches the reference stored in the leaf.
                    let _ = value_log.read(offset, len)?;
                }
            }
            current_id = page.next_page_id()?;
        }

        value_log.validate_refs(&observed)
    }

    // ------------------------------------------------------------------
    // Integrity check
    // ------------------------------------------------------------------

    fn check_node(
        &self,
        page: &crate::page::Page,
        seen: &mut HashSet<PageId>,
        leaf_prev: &mut Option<PageId>,
        is_root: bool,
        low: Option<&[u8]>,
        high: Option<&[u8]>,
    ) -> Result<()> {
        let count = page.live_count()?;

        if page.is_leaf() {
            if count == 0 && !is_root {
                return Err(Error::Corruption(format!(
                    "non-root leaf page {} is empty",
                    page.id
                )));
            }

            let prev = page.prev_page_id()?;
            let next = page.next_page_id()?;
            if let Some(expected_prev) = *leaf_prev {
                if prev != expected_prev {
                    return Err(Error::Corruption(format!(
                        "leaf page {} prev mismatch: expected {expected_prev}, got {prev}",
                        page.id
                    )));
                }
            } else if prev != NULL_PAGE_ID {
                return Err(Error::Corruption(format!(
                    "leftmost leaf page {} has a prev pointer",
                    page.id
                )));
            }
            *leaf_prev = Some(page.id);

            let mut last: Option<Vec<u8>> = None;
            for idx in 0..page.slot_count()? {
                if page.read_slot(idx)?.is_deleted() {
                    continue;
                }
                let cell = page.get_by_slot(idx)?;
                if let Some(low) = low
                    && cell.key.as_slice() < low
                {
                    return Err(Error::Corruption(format!(
                        "leaf page {} key {:?} below low bound {:?}",
                        page.id, cell.key, low
                    )));
                }
                if let Some(high) = high
                    && cell.key.as_slice() >= high
                {
                    return Err(Error::Corruption(format!(
                        "leaf page {} key {:?} at or above high bound {:?}",
                        page.id, cell.key, high
                    )));
                }
                if let Some(ref last) = last
                    && cell.key.as_slice() <= last.as_slice()
                {
                    return Err(Error::Corruption(format!(
                        "leaf page {} keys out of order",
                        page.id
                    )));
                }
                last = Some(cell.key);
            }

            if next != NULL_PAGE_ID {
                let next_guard = self.pool.fetch_or_read(next)?;
                let next_page = next_guard.page();
                if !next_page.is_leaf() {
                    return Err(Error::Corruption(format!(
                        "page {next} referenced as leaf next is not a leaf"
                    )));
                }
                if next_page.prev_page_id()? != page.id {
                    return Err(Error::Corruption(format!(
                        "leaf page {} next {next} has prev {} instead of {}",
                        next,
                        next_page.prev_page_id()?,
                        page.id
                    )));
                }
            }
            return Ok(());
        }

        // Internal node.
        if count == 0 && !is_root {
            return Err(Error::Corruption(format!(
                "non-root internal page {} is empty",
                page.id
            )));
        }
        let leftmost = page.leftmost_child()?;
        if leftmost == NULL_PAGE_ID {
            return Err(Error::Corruption(format!(
                "internal page {} has no leftmost child",
                page.id
            )));
        }
        if !seen.insert(leftmost) {
            return Err(Error::Corruption(format!(
                "page {leftmost} already visited (cycle) from internal page {}",
                page.id
            )));
        }

        let mut last_key: Option<Vec<u8>> = None;
        for idx in 0..page.slot_count()? {
            if page.read_slot(idx)?.is_deleted() {
                continue;
            }
            let cell = page.get_by_slot(idx)?;
            let child_id = decode_page_id(&cell.value.as_value_kind())?;
            if !seen.insert(child_id) {
                return Err(Error::Corruption(format!(
                    "page {child_id} already visited (cycle) from internal page {}",
                    page.id
                )));
            }
            if let Some(ref last) = last_key
                && cell.key.as_slice() <= last.as_slice()
            {
                return Err(Error::Corruption(format!(
                    "internal page {} keys out of order",
                    page.id
                )));
            }
            last_key = Some(cell.key);
        }

        // Collect live separators so the page borrow can be dropped before we
        // recurse and fetch children from the buffer pool.
        let mut entries: Vec<(Vec<u8>, PageId)> = Vec::new();
        for idx in 0..page.slot_count()? {
            if page.read_slot(idx)?.is_deleted() {
                continue;
            }
            let cell = page.get_by_slot(idx)?;
            entries.push((cell.key, decode_page_id(&cell.value.as_value_kind())?));
        }

        let left_high = entries.first().map(|(k, _)| k.as_slice()).or(high);
        let guard = self.pool.fetch_or_read(leftmost)?;
        self.check_node(guard.page(), seen, leaf_prev, false, low, left_high)?;

        for i in 0..entries.len() {
            let (key, child_id) = &entries[i];
            let child_high = entries.get(i + 1).map(|(k, _)| k.as_slice()).or(high);
            let guard = self.pool.fetch_or_read(*child_id)?;
            self.check_node(
                guard.page(),
                seen,
                leaf_prev,
                false,
                Some(key.as_slice()),
                child_high,
            )?;
        }

        Ok(())
    }

    // The single-threaded merge/redistribute path that used to live here has
    // been removed.  The production path is the lock-coupled OLC implementation
    // above, which is the only path that writes WAL records for structure
    // modifications.
}

// ----------------------------------------------------------------------
// Page-level helpers
// ----------------------------------------------------------------------

fn encode_page_id(id: PageId) -> [u8; 8] {
    id.to_le_bytes()
}

fn decode_page_id(value: &ValueKind<'_>) -> Result<PageId> {
    match value {
        ValueKind::Inline(b) if b.len() == 8 => Ok(PageId::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ])),
        _ => Err(Error::Corruption(
            "internal node value is not an 8-byte page id".into(),
        )),
    }
}

pub(crate) fn child_for_key(page: &crate::page::Page, key: &[u8]) -> Result<PageId> {
    let count = page.slot_count()?;
    let mut last_live_child: Option<PageId> = None;
    for idx in 0..count {
        if page.read_slot(idx)?.is_deleted() {
            continue;
        }
        let cell = page.get_by_slot(idx)?;
        if key < cell.key.as_slice() {
            return if let Some(child) = last_live_child {
                Ok(child)
            } else {
                page.leftmost_child()
            };
        }
        last_live_child = Some(decode_page_id(&cell.value.as_value_kind())?);
    }
    if let Some(child) = last_live_child {
        Ok(child)
    } else {
        page.leftmost_child()
    }
}

fn child_position(page: &crate::page::Page, child_id: PageId) -> Result<Option<usize>> {
    if page.leftmost_child()? == child_id {
        return Ok(Some(0));
    }
    let count = page.slot_count()?;
    for idx in 0..count {
        if page.read_slot(idx)?.is_deleted() {
            continue;
        }
        let cell = page.get_by_slot(idx)?;
        if decode_page_id(&cell.value.as_value_kind())? == child_id {
            return Ok(Some(idx + 1));
        }
    }
    Ok(None)
}

fn child_at(page: &crate::page::Page, pos: usize) -> Result<PageId> {
    if pos == 0 {
        page.leftmost_child()
    } else {
        decode_page_id(&page.get_by_slot(pos - 1)?.value.as_value_kind())
    }
}

fn right_child_at(page: &crate::page::Page, pos: usize) -> Result<Option<PageId>> {
    let count = page.slot_count()?;
    if pos < count {
        Ok(Some(child_at(page, pos + 1)?))
    } else {
        Ok(None)
    }
}

fn can_give_cell(page: &crate::page::Page, min_cells: usize) -> bool {
    page.live_count().is_ok_and(|n| n > min_cells)
}

fn split_leaf(left: &crate::page::Page, right: &crate::page::Page) -> Result<Vec<u8>> {
    left.compact()?;
    right.compact()?;
    right.set_leaf();
    let count = left.slot_count()?;
    if count < 2 {
        return Err(Error::Corruption(
            "cannot split a leaf with fewer than 2 cells".into(),
        ));
    }
    let mid = count / 2;

    let mut moved: Vec<OwnedCell> = Vec::new();
    for idx in mid..count {
        let cell = left.get_by_slot(idx)?;
        moved.push(cell);
    }

    for idx in (mid..count).rev() {
        left.write_slot(idx, Slot::deleted());
    }
    left.compact()?;

    for cell in moved {
        right.insert_with_mvcc(&cell.key, &cell.value.as_value_kind(), cell.mvcc.as_ref())?;
    }

    if right.slot_count()? == 0 {
        return Err(Error::Corruption(
            "split produced an empty right page".into(),
        ));
    }
    let separator = right.get_by_slot(0)?.key;
    Ok(separator)
}

fn split_internal(left: &crate::page::Page, right: &crate::page::Page) -> Result<Vec<u8>> {
    left.compact()?;
    right.compact()?;
    right.set_internal();
    let count = left.slot_count()?;
    if count < 2 {
        return Err(Error::Corruption(
            "cannot split an internal page with fewer than 2 cells".into(),
        ));
    }
    let mid = count / 2;

    let promoted_cell = left.get_by_slot(mid)?;
    let promoted_key = promoted_cell.key;
    let right_leftmost = decode_page_id(&promoted_cell.value.as_value_kind())?;

    let mut moved: Vec<(Vec<u8>, PageId)> = Vec::new();
    for idx in (mid + 1)..count {
        let cell = left.get_by_slot(idx)?;
        moved.push((cell.key, decode_page_id(&cell.value.as_value_kind())?));
    }

    for idx in (mid..count).rev() {
        left.write_slot(idx, Slot::deleted());
    }
    left.compact()?;

    right.set_leftmost_child(right_leftmost);
    for (key, child_id) in moved {
        let bytes = encode_page_id(child_id);
        right.insert(&key, &ValueKind::Inline(&bytes))?;
    }

    Ok(promoted_key)
}

fn merge_leaf_right_into_left(
    left: &crate::page::Page,
    right: &crate::page::Page,
    pool: &Arc<BufferPool>,
) -> Result<()> {
    left.compact()?;
    right.compact()?;
    let count = right.slot_count()?;
    for idx in 0..count {
        let cell = right.get_by_slot(idx)?;
        left.insert_with_mvcc(&cell.key, &cell.value.as_value_kind(), cell.mvcc.as_ref())?;
    }

    let right_next = right.next_page_id()?;
    left.set_next_page_id(right_next);

    if right_next != NULL_PAGE_ID {
        let next_guard = pool.fetch_or_read(right_next)?;
        next_guard.page().set_prev_page_id(left.id);
        next_guard.mark_dirty();
    }
    Ok(())
}

fn merge_internal_right_into_left(
    left: &crate::page::Page,
    right: &crate::page::Page,
    separator: &[u8],
) -> Result<()> {
    left.compact()?;
    right.compact()?;

    let right_leftmost = right.leftmost_child()?;
    let bytes = encode_page_id(right_leftmost);
    left.insert(separator, &ValueKind::Inline(&bytes))?;

    let count = right.slot_count()?;
    for idx in 0..count {
        let cell = right.get_by_slot(idx)?;
        let child_id = decode_page_id(&cell.value.as_value_kind())?;
        let bytes = encode_page_id(child_id);
        left.insert(&cell.key, &ValueKind::Inline(&bytes))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::BufferPool;
    use crate::disk::PagedFile;
    use crate::space::PageAllocator;
    use crate::sync::Mutex as SyncMutex;
    use crate::txn::NULL_TS;

    #[cfg(not(miri))]
    use crate::checkpoint::{Checkpoint, Meta};

    fn make_tree(page_size: usize, min_cells: usize) -> (BPlusTree, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let disk = Arc::new(PagedFile::open(dir.path().join("pages.dat"), page_size).unwrap());
        let alloc = Arc::new(SyncMutex::new(PageAllocator::new(PageId::new(1))));
        let pool = Arc::new(BufferPool::new(64, page_size, disk, alloc).unwrap());
        let tree = BPlusTree::with_min_cells(pool, page_size / 4, min_cells).unwrap();
        (tree, dir)
    }

    fn make_tree_with_wal(
        page_size: usize,
        min_cells: usize,
    ) -> (
        BPlusTree,
        Arc<BufferPool>,
        Arc<WalLog>,
        Arc<SyncMutex<PageAllocator>>,
        tempfile::TempDir,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let disk = Arc::new(PagedFile::open(dir.path().join("pages.dat"), page_size).unwrap());
        let alloc = Arc::new(SyncMutex::new(PageAllocator::new(PageId::new(1))));
        let pool = Arc::new(BufferPool::new(64, page_size, disk, alloc.clone()).unwrap());
        let wal = Arc::new(WalLog::open(dir.path(), storage_wal::WalOptions::default()).unwrap());
        pool.set_wal(Arc::clone(&wal));
        let tree = BPlusTree::with_min_cells(pool.clone(), page_size / 4, min_cells)
            .unwrap()
            .with_wal(Arc::clone(&wal));
        (tree, pool, wal, alloc, dir)
    }

    #[allow(clippy::type_complexity)]
    fn make_tree_with_wal_and_value_log(
        page_size: usize,
        min_cells: usize,
    ) -> (
        BPlusTree,
        Arc<BufferPool>,
        Arc<WalLog>,
        Arc<ValueLog>,
        Arc<SyncMutex<PageAllocator>>,
        tempfile::TempDir,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let disk = Arc::new(PagedFile::open(dir.path().join("pages.dat"), page_size).unwrap());
        let alloc = Arc::new(SyncMutex::new(PageAllocator::new(PageId::new(1))));
        let pool = Arc::new(BufferPool::new(64, page_size, disk, alloc.clone()).unwrap());
        let wal = Arc::new(WalLog::open(dir.path(), storage_wal::WalOptions::default()).unwrap());
        pool.set_wal(Arc::clone(&wal));
        let value_log = Arc::new(ValueLog::open(dir.path()).unwrap());
        let tree = BPlusTree::with_min_cells(pool.clone(), page_size / 4, min_cells)
            .unwrap()
            .with_wal(Arc::clone(&wal))
            .with_value_log(Arc::clone(&value_log));
        (tree, pool, wal, value_log, alloc, dir)
    }

    #[test]
    fn empty_tree_get() {
        let (tree, _dir) = make_tree(512, 1);
        assert!(tree.get(b"a").unwrap().is_none());
    }

    #[test]
    fn insert_and_get_single() {
        let (tree, _dir) = make_tree(512, 1);
        tree.insert(b"k", b"v").unwrap();
        assert_eq!(tree.get(b"k").unwrap(), Some(b"v".to_vec()));
        assert!(tree.get(b"x").unwrap().is_none());
        tree.check_integrity().unwrap();
    }

    #[test]
    #[cfg(not(miri))]
    fn insert_many_triggers_root_split() {
        let (tree, _dir) = make_tree(512, 1);
        for i in 0u64..50 {
            let key = format!("{:08x}", i);
            tree.insert(key.as_bytes(), key.as_bytes()).unwrap();
            tree.check_integrity().unwrap();
        }
        for i in 0u64..50 {
            let key = format!("{:08x}", i);
            assert_eq!(
                tree.get(key.as_bytes()).unwrap(),
                Some(key.as_bytes().to_vec())
            );
        }
    }

    #[test]
    fn delete_existing_key() {
        let (tree, _dir) = make_tree(512, 1);
        tree.insert(b"a", b"1").unwrap();
        tree.insert(b"b", b"2").unwrap();
        assert!(tree.delete(b"a").unwrap());
        assert!(tree.get(b"a").unwrap().is_none());
        assert_eq!(tree.get(b"b").unwrap(), Some(b"2".to_vec()));
        tree.check_integrity().unwrap();
    }

    #[test]
    fn delete_nonexistent_key() {
        let (tree, _dir) = make_tree(512, 1);
        tree.insert(b"a", b"1").unwrap();
        assert!(!tree.delete(b"z").unwrap());
        tree.check_integrity().unwrap();
    }

    #[test]
    #[cfg(not(miri))]
    fn insert_delete_all_keys() {
        let (tree, _dir) = make_tree(512, 1);
        let keys: Vec<String> = (0u64..60).map(|i| format!("{:08x}", i)).collect();
        for key in &keys {
            tree.insert(key.as_bytes(), b"v").unwrap();
        }
        // Deterministic pseudo-shuffle: reverse every adjacent pair.
        let mut shuffled = keys.clone();
        for i in (0..shuffled.len().saturating_sub(1)).step_by(2) {
            shuffled.swap(i, i + 1);
        }
        for key in &shuffled {
            let deleted = tree.delete(key.as_bytes()).unwrap();
            if !deleted {
                panic!(
                    "failed to delete key {key}: get={:?}",
                    tree.get(key.as_bytes())
                );
            }
            tree.check_integrity().unwrap();
        }
        for key in &keys {
            assert!(tree.get(key.as_bytes()).unwrap().is_none());
        }
    }

    #[test]
    #[cfg(not(miri))]
    fn insert_delete_reverse_order() {
        let (tree, _dir) = make_tree(512, 1);
        let keys: Vec<String> = (0u64..40).map(|i| format!("{:08x}", i)).collect();
        for key in &keys {
            tree.insert(key.as_bytes(), b"v").unwrap();
        }
        for key in keys.iter().rev() {
            assert!(tree.delete(key.as_bytes()).unwrap());
            tree.check_integrity().unwrap();
        }
    }

    #[test]
    fn insert_persists_after_flush() {
        let (tree, dir) = make_tree(512, 1);
        for i in 0u64..30 {
            let key = format!("{:08x}", i);
            tree.insert(key.as_bytes(), key.as_bytes()).unwrap();
        }
        tree.pool.flush_all().unwrap();

        // Reopen the pool and validate the tree still works.
        let disk = Arc::new(PagedFile::open(dir.path().join("pages.dat"), 512).unwrap());
        let alloc = Arc::new(SyncMutex::new(PageAllocator::new(PageId::new(1))));
        let pool2 = Arc::new(BufferPool::new(64, 512, disk, alloc).unwrap());
        let tree2 = BPlusTree {
            pool: pool2,
            root_page_id: AtomicU64::new(tree.root_page_id().get()),
            inline_threshold: 128,
            min_cells: 1,
            retired: SyncMutex::new(Vec::new()),
            active_roots: SyncMutex::new(HashMap::new()),
            wal: None,
            txn_table: Arc::new(TransactionTable::new()),
            value_log: None,
        };
        tree2.check_integrity().unwrap();
        for i in 0u64..30 {
            let key = format!("{:08x}", i);
            assert_eq!(
                tree2.get(key.as_bytes()).unwrap(),
                Some(key.as_bytes().to_vec())
            );
        }
    }

    #[test]
    fn duplicate_insert_overwrites() {
        let (tree, _dir) = make_tree(512, 1);
        tree.insert(b"k", b"v1").unwrap();
        tree.insert(b"k", b"v2").unwrap();
        assert_eq!(tree.get(b"k").unwrap(), Some(b"v2".to_vec()));
    }

    #[test]
    fn large_value_rejected_without_value_log() {
        let (tree, _dir) = make_tree(512, 1);
        let big = vec![b'x'; 256];
        assert!(matches!(
            tree.insert(b"k", &big),
            Err(Error::Unsupported("value log not configured"))
        ));
    }

    #[test]
    fn empty_key_roundtrip() {
        let (tree, _dir) = make_tree(512, 1);
        tree.insert(b"", b"v").unwrap();
        assert_eq!(tree.get(b"").unwrap(), Some(b"v".to_vec()));
        assert!(tree.delete(b"").unwrap());
        assert_eq!(tree.get(b"").unwrap(), None);
        tree.check_integrity().unwrap();
    }

    #[test]
    #[cfg(not(miri))]
    fn delete_contiguous_retires_pages() {
        let (tree, _dir) = make_tree(512, 1);
        for i in 0u64..500 {
            let key = format!("k{:08}", i);
            tree.insert(key.as_bytes(), b"v").unwrap();
        }
        tree.check_integrity().unwrap();
        for i in 250..500u64 {
            let key = format!("k{:08}", i);
            tree.delete(key.as_bytes()).unwrap();
        }
        tree.check_integrity().unwrap();
        assert!(
            tree.retired_count() > 0,
            "expected retired pages after deleting half the keys"
        );
    }

    #[test]
    #[cfg(not(miri))]
    fn concurrent_insert_and_get_stress() {
        let (tree, _dir) = make_tree(4096, 1);
        let tree = Arc::new(tree);
        let num_threads = 2;
        let keys_per_thread = 100;

        let handles: Vec<_> = (0..num_threads)
            .map(|t| {
                let tree = Arc::clone(&tree);
                std::thread::spawn(move || {
                    for i in 0..keys_per_thread {
                        let key = format!("t{t}-{:08x}", i);
                        tree.insert(key.as_bytes(), key.as_bytes()).unwrap();
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        tree.check_integrity().unwrap();
        for t in 0..num_threads {
            for i in 0..keys_per_thread {
                let key = format!("t{t}-{:08x}", i);
                assert_eq!(
                    tree.get(key.as_bytes()).unwrap(),
                    Some(key.as_bytes().to_vec()),
                    "missing key {key}"
                );
            }
        }
    }

    #[test]
    #[cfg(not(miri))]
    fn concurrent_two_thread_insert() {
        let (tree, _dir) = make_tree(512, 1);
        let tree = Arc::new(tree);
        let h1 = {
            let tree = Arc::clone(&tree);
            std::thread::spawn(move || {
                for i in 0u64..20 {
                    let key = format!("a{:08x}", i);
                    tree.insert(key.as_bytes(), b"v").unwrap();
                }
            })
        };
        let h2 = {
            let tree = Arc::clone(&tree);
            std::thread::spawn(move || {
                for i in 0u64..20 {
                    let key = format!("b{:08x}", i);
                    tree.insert(key.as_bytes(), b"v").unwrap();
                }
            })
        };
        h1.join().unwrap();
        h2.join().unwrap();
        tree.check_integrity().unwrap();
    }

    #[test]
    #[cfg(not(miri))]
    fn single_thread_delete_every_other_key() {
        let (tree, _dir) = make_tree(512, 1);
        let keys: Vec<String> = (0u64..80).map(|i| format!("{:08x}", i)).collect();
        for key in &keys {
            tree.insert(key.as_bytes(), b"v").unwrap();
        }
        for key in keys.iter().step_by(2) {
            tree.delete(key.as_bytes()).unwrap();
            tree.check_integrity().unwrap();
        }
        for key in keys.iter().skip(1).step_by(2) {
            tree.delete(key.as_bytes()).unwrap();
            tree.check_integrity().unwrap();
        }
    }

    #[test]
    #[cfg(not(miri))]
    fn concurrent_delete_stress() {
        let (tree, _dir) = make_tree(512, 1);
        let tree = Arc::new(tree);
        let keys: Vec<String> = (0u64..40).map(|i| format!("{:08x}", i)).collect();
        for key in &keys {
            tree.insert(key.as_bytes(), b"v").unwrap();
        }

        let h1 = {
            let tree = Arc::clone(&tree);
            let keys = keys.clone();
            std::thread::spawn(move || {
                for key in keys.iter().step_by(2) {
                    tree.delete(key.as_bytes()).unwrap();
                }
            })
        };
        let h2 = {
            let tree = Arc::clone(&tree);
            let keys = keys.clone();
            std::thread::spawn(move || {
                for key in keys.iter().skip(1).step_by(2) {
                    tree.delete(key.as_bytes()).unwrap();
                }
            })
        };
        h1.join().unwrap();
        h2.join().unwrap();

        tree.check_integrity().unwrap();
        for key in &keys {
            assert!(
                tree.get(key.as_bytes()).unwrap().is_none(),
                "key {key} still present after concurrent delete"
            );
        }
    }

    #[test]
    #[cfg(not(miri))]
    fn concurrent_mixed_insert_delete_stress() {
        let (tree, _dir) = make_tree(512, 1);
        let tree = Arc::new(tree);
        let num_threads = 2;
        let keys_per_thread = 60;

        // Pre-populate so the deleter has work to do.
        for t in 0..num_threads {
            for i in 0..keys_per_thread {
                let key = format!("t{t}-{:08x}", i);
                tree.insert(key.as_bytes(), key.as_bytes()).unwrap();
            }
        }

        let h_insert = {
            let tree = Arc::clone(&tree);
            std::thread::spawn(move || {
                for t in 0..num_threads {
                    for i in 0..keys_per_thread {
                        let key = format!("t{t}-{:08x}", i + keys_per_thread);
                        tree.insert(key.as_bytes(), key.as_bytes()).unwrap();
                    }
                }
            })
        };
        let h_delete = {
            let tree = Arc::clone(&tree);
            std::thread::spawn(move || {
                for t in 0..num_threads {
                    for i in 0..keys_per_thread {
                        let key = format!("t{t}-{:08x}", i);
                        tree.delete(key.as_bytes()).unwrap();
                    }
                }
            })
        };
        h_insert.join().unwrap();
        h_delete.join().unwrap();

        tree.check_integrity().unwrap();
        for t in 0..num_threads {
            for i in 0..keys_per_thread {
                let old_key = format!("t{t}-{:08x}", i);
                assert!(
                    tree.get(old_key.as_bytes()).unwrap().is_none(),
                    "old key {old_key} still present"
                );
                let new_key = format!("t{t}-{:08x}", i + keys_per_thread);
                assert_eq!(
                    tree.get(new_key.as_bytes()).unwrap(),
                    Some(new_key.as_bytes().to_vec()),
                    "new key {new_key} missing"
                );
            }
        }
    }

    /// Regression test for the poisoned-old-root race.
    ///
    /// When the root collapses due to concurrent deletes, the old root page is
    /// poisoned (`leftmost_child` set to NULL) so that no thread follows it.
    /// Optimistic traversals that captured the old root id before the collapse
    /// must retry rather than fetch a null page id.
    #[test]
    #[cfg(not(miri))]
    fn concurrent_delete_and_scan_stress() {
        let (tree, _dir) = make_tree(512, 1);
        let tree = Arc::new(tree);
        let keys: Vec<String> = (0u64..120).map(|i| format!("{:08x}", i)).collect();
        for key in &keys {
            tree.insert(key.as_bytes(), b"v").unwrap();
        }

        let mut handles = Vec::new();

        // Two deleters remove disjoint key subsets.  This drives root-level
        // merges that poison the old root.
        for t in 0..2 {
            let tree = Arc::clone(&tree);
            let keys = keys.clone();
            handles.push(std::thread::spawn(move || {
                for key in keys.iter().skip(t).step_by(2) {
                    tree.delete(key.as_bytes()).unwrap();
                }
            }));
        }

        // One scanner repeatedly traverses the tree from the current root while
        // deletes are collapsing it.  This exercises both `optimistic_path_to_leaf`
        // and `BPlusTreeCursor::descend`.
        let scanner_tree = Arc::clone(&tree);
        handles.push(std::thread::spawn(move || {
            for _ in 0..30 {
                let txn = scanner_tree
                    .begin_txn(crate::txn::IsolationLevel::Snapshot)
                    .unwrap();
                let cursor = crate::cursor::BPlusTreeCursor::new(
                    scanner_tree.clone(),
                    &txn,
                    None,
                    None,
                    None,
                )
                .unwrap();
                let _count = cursor.count();
            }
        }));

        for h in handles {
            h.join().unwrap();
        }

        tree.check_integrity().unwrap();
    }

    #[test]
    fn simple_insert_writes_wal_record() {
        let (tree, dir) = make_tree(4096, 1);
        let wal = Arc::new(WalLog::open(dir.path(), storage_wal::WalOptions::default()).unwrap());
        let tree = tree.with_wal(Arc::clone(&wal));

        tree.insert(b"hello", b"world").unwrap();

        let records: Vec<_> = wal
            .iter(NULL_LSN)
            .unwrap()
            .collect::<Result<Vec<_>>>()
            .unwrap();
        assert!(
            records.iter().any(|(_lsn, r)| matches!(
                r,
                crate::wal::Record {
                    header: crate::wal::RecordHeader { record_type: crate::wal::RecordType::InsertCell, .. },
                    payload: crate::wal::RecordPayload::InsertCell { cell },
                } if cell.key == b"hello"
            )),
            "expected an InsertCell WAL record for key 'hello', got {:?}",
            records
        );
    }

    #[test]
    fn wal_records_are_replayable() {
        let (tree, dir) = make_tree(4096, 1);
        let wal = Arc::new(WalLog::open(dir.path(), storage_wal::WalOptions::default()).unwrap());
        let tree = tree.with_wal(Arc::clone(&wal));

        tree.insert(b"hello", b"world").unwrap();
        tree.insert(b"foo", b"bar").unwrap();
        tree.delete(b"hello").unwrap();

        // Reopen a fresh pool and replay the WAL.
        let disk = Arc::new(PagedFile::open(dir.path().join("pages.dat"), 4096).unwrap());
        let alloc = Arc::new(SyncMutex::new(PageAllocator::new(PageId::new(1))));
        let pool2 = Arc::new(BufferPool::new(64, 4096, disk, alloc).unwrap());
        let fresh_root_guard = pool2.new_page().unwrap();
        let fresh_root = fresh_root_guard.page().id;
        fresh_root_guard.page().set_leaf();
        fresh_root_guard.mark_dirty();
        drop(fresh_root_guard);

        let recovery = crate::recovery::Recovery::new(pool2.clone(), wal, fresh_root);
        let root = recovery.recover(NULL_LSN).unwrap();
        assert_eq!(root, fresh_root);

        // Recovery replayed cell-level records on the fresh root page.
        let guard = pool2.fetch_or_read(root).unwrap();
        assert!(guard.page().get(b"hello").unwrap().is_none());
        let value = guard.page().get(b"foo").unwrap().map(|c| match c.value {
            crate::slot::OwnedValue::Inline(v) => v,
            _ => panic!("expected inline value"),
        });
        assert_eq!(value, Some(b"bar".to_vec()));
    }

    #[test]
    #[cfg(not(miri))]
    fn wal_recovery_replays_split() {
        // Use a small page size so a handful of inserts triggers a root split.
        let (tree, dir) = make_tree(512, 1);
        let wal = Arc::new(WalLog::open(dir.path(), storage_wal::WalOptions::default()).unwrap());
        let tree = tree.with_wal(Arc::clone(&wal));

        let keys: Vec<String> = (0u64..40).map(|i| format!("{:08x}", i)).collect();
        for key in &keys {
            tree.insert(key.as_bytes(), key.as_bytes()).unwrap();
        }
        let original_root = tree.root_page_id();

        // Reopen a fresh pool and replay the WAL from an empty root.
        let disk = Arc::new(PagedFile::open(dir.path().join("pages.dat"), 512).unwrap());
        let alloc = Arc::new(SyncMutex::new(PageAllocator::new(PageId::new(1))));
        let pool2 = Arc::new(BufferPool::new(64, 512, disk, alloc).unwrap());
        let fresh_root_guard = pool2.new_page().unwrap();
        let fresh_root = fresh_root_guard.page().id;
        fresh_root_guard.page().set_leaf();
        fresh_root_guard.mark_dirty();
        drop(fresh_root_guard);

        let recovery = crate::recovery::Recovery::new(pool2.clone(), wal, fresh_root);
        let recovered_root = recovery.recover(NULL_LSN).unwrap();

        // The recovered root must match the original tree's root, and all keys
        // must be present.
        assert_eq!(recovered_root, original_root);
        let tree2 = BPlusTree::open(pool2, recovered_root, 512 / 4);
        for key in &keys {
            assert_eq!(
                tree2.get(key.as_bytes()).unwrap(),
                Some(key.as_bytes().to_vec()),
                "key {key} missing after recovery on root {recovered_root}"
            );
        }
    }

    #[test]
    #[cfg(not(miri))]
    fn wal_recovery_replays_merges_after_checkpoint() {
        // Build a tree with WAL, insert enough keys to split, then checkpoint.
        let (tree, pool, wal, alloc, dir) = make_tree_with_wal(512, 1);
        let keys: Vec<String> = (0u64..40).map(|i| format!("{:08x}", i)).collect();
        for key in &keys {
            tree.insert(key.as_bytes(), key.as_bytes()).unwrap();
        }
        let original_root = tree.root_page_id();
        tree.check_integrity().unwrap();

        let root_arc = Arc::new(std::sync::atomic::AtomicU64::new(original_root.get()));
        let cp = Checkpoint::new(
            dir.path(),
            pool.clone(),
            wal.clone(),
            root_arc,
            alloc.clone(),
        );
        let meta = cp.run().unwrap();
        assert!(meta.checkpoint_lsn > NULL_LSN);
        drop(cp);

        // Delete most keys to trigger leaf merges, internal underflow, and
        // eventually a root shrink.
        let remaining: Vec<String> = keys.iter().step_by(10).cloned().collect();
        for key in &keys {
            if !remaining.contains(key) {
                tree.delete(key.as_bytes()).unwrap();
            }
        }
        tree.check_integrity().unwrap();

        // Reopen from the checkpoint and replay the WAL.  Drop the original
        // tree, pool and WAL first so the directory lock is released.
        let meta = Meta::read(dir.path()).unwrap().unwrap();
        drop(tree);
        drop(pool);
        drop(wal);
        drop(alloc);

        let disk = Arc::new(PagedFile::open(dir.path().join("pages.dat"), 512).unwrap());
        let pool2 = Arc::new(
            BufferPool::new(
                64,
                512,
                disk,
                Arc::new(SyncMutex::new(meta.allocator.clone())),
            )
            .unwrap(),
        );
        let wal2 = Arc::new(WalLog::open(dir.path(), storage_wal::WalOptions::default()).unwrap());

        let recovery = crate::recovery::Recovery::new(pool2.clone(), wal2, meta.root_page_id);
        let recovered_root = recovery.recover(meta.checkpoint_lsn).unwrap();

        let tree2 = BPlusTree::open(pool2, recovered_root, 512 / 4);
        for key in &remaining {
            assert_eq!(
                tree2.get(key.as_bytes()).unwrap(),
                Some(key.as_bytes().to_vec()),
                "remaining key {key} missing after recovery on root {recovered_root}"
            );
        }
        for key in &keys {
            if !remaining.contains(key) {
                assert!(
                    tree2.get(key.as_bytes()).unwrap().is_none(),
                    "deleted key {key} still present after recovery"
                );
            }
        }
    }

    #[test]
    fn recover_after_wal_fsync_before_page_flush() {
        // Simulate a crash after the WAL record is durable but before the
        // buffer-pool page is flushed to disk.  Recovery must redo the insert.
        let (tree, dir) = make_tree(512, 1);
        let wal = Arc::new(WalLog::open(dir.path(), storage_wal::WalOptions::default()).unwrap());
        let tree = tree.with_wal(Arc::clone(&wal));

        tree.insert(b"a", b"1").unwrap();
        tree.insert(b"b", b"2").unwrap();

        // Drop without flushing dirty pages.  The WAL has already fsynced.
        drop(tree);
        drop(wal);

        let disk = Arc::new(PagedFile::open(dir.path().join("pages.dat"), 512).unwrap());
        let alloc = Arc::new(SyncMutex::new(PageAllocator::new(PageId::new(1))));
        let pool2 = Arc::new(BufferPool::new(64, 512, disk, alloc).unwrap());
        let wal2 = Arc::new(WalLog::open(dir.path(), storage_wal::WalOptions::default()).unwrap());

        // Recovery needs an initial leaf root page to replay cell-level records
        // into when the original page was never flushed.
        let fresh_root_guard = pool2.new_page().unwrap();
        let fresh_root = fresh_root_guard.page().id;
        fresh_root_guard.page().set_leaf();
        fresh_root_guard.mark_dirty();
        drop(fresh_root_guard);

        let recovery = crate::recovery::Recovery::new(pool2.clone(), wal2, fresh_root);
        let root = recovery.recover(NULL_LSN).unwrap();

        let tree2 = BPlusTree::open(pool2, root, 512 / 4);
        assert_eq!(tree2.get(b"a").unwrap(), Some(b"1".to_vec()));
        assert_eq!(tree2.get(b"b").unwrap(), Some(b"2".to_vec()));
    }

    #[test]
    #[cfg(not(miri))]
    fn recover_after_checkpoint_truncate() {
        // Insert keys, checkpoint (which flushes dirty pages and truncates old
        // WAL segments), insert more keys, then reopen and recover from META.
        let (tree, pool, wal, alloc, dir) = make_tree_with_wal(512, 1);
        let keys: Vec<String> = (0u64..40).map(|i| format!("{:08x}", i)).collect();
        for key in &keys {
            tree.insert(key.as_bytes(), key.as_bytes()).unwrap();
        }
        let original_root = tree.root_page_id();
        tree.check_integrity().unwrap();

        let root_arc = Arc::new(std::sync::atomic::AtomicU64::new(original_root.get()));
        let cp = Checkpoint::new(
            dir.path(),
            pool.clone(),
            wal.clone(),
            root_arc,
            alloc.clone(),
        );
        let meta = cp.run().unwrap();
        assert!(meta.checkpoint_lsn > NULL_LSN);
        drop(cp);

        // These post-checkpoint inserts are only in the WAL (and buffer pool).
        let extra: Vec<String> = (40u64..50).map(|i| format!("{:08x}", i)).collect();
        for key in &extra {
            tree.insert(key.as_bytes(), key.as_bytes()).unwrap();
        }

        // Reopen from the checkpoint META and replay the WAL.  Drop the
        // original tree, pool and WAL so the directory lock is released.
        let meta = Meta::read(dir.path()).unwrap().unwrap();
        drop(tree);
        drop(pool);
        drop(wal);
        drop(alloc);

        let disk = Arc::new(PagedFile::open(dir.path().join("pages.dat"), 512).unwrap());
        let pool2 = Arc::new(
            BufferPool::new(
                64,
                512,
                disk,
                Arc::new(SyncMutex::new(meta.allocator.clone())),
            )
            .unwrap(),
        );
        let wal2 = Arc::new(WalLog::open(dir.path(), storage_wal::WalOptions::default()).unwrap());

        let recovery = crate::recovery::Recovery::new(pool2.clone(), wal2, meta.root_page_id);
        let recovered_root = recovery.recover(meta.checkpoint_lsn).unwrap();

        let tree2 = BPlusTree::open(pool2, recovered_root, 512 / 4);
        tree2.check_integrity().unwrap();
        for key in keys.iter().chain(&extra) {
            assert_eq!(
                tree2.get(key.as_bytes()).unwrap(),
                Some(key.as_bytes().to_vec()),
                "key {key} missing after checkpoint recovery"
            );
        }
    }

    #[test]
    fn recovery_is_idempotent() {
        // Running recovery twice on the same state must not change the result.
        let (tree, dir) = make_tree(512, 1);
        let wal = Arc::new(WalLog::open(dir.path(), storage_wal::WalOptions::default()).unwrap());
        let tree = tree.with_wal(Arc::clone(&wal));

        tree.insert(b"x", b"9").unwrap();
        drop(tree);
        drop(wal);

        let disk = Arc::new(PagedFile::open(dir.path().join("pages.dat"), 512).unwrap());
        let alloc = Arc::new(SyncMutex::new(PageAllocator::new(PageId::new(1))));
        let pool2 = Arc::new(BufferPool::new(64, 512, disk, alloc).unwrap());
        let wal2 = Arc::new(WalLog::open(dir.path(), storage_wal::WalOptions::default()).unwrap());

        let fresh_root_guard = pool2.new_page().unwrap();
        let fresh_root = fresh_root_guard.page().id;
        fresh_root_guard.page().set_leaf();
        fresh_root_guard.mark_dirty();
        drop(fresh_root_guard);

        let recovery = crate::recovery::Recovery::new(pool2.clone(), wal2.clone(), fresh_root);
        let root1 = recovery.recover(NULL_LSN).unwrap();
        let root2 = recovery.recover(NULL_LSN).unwrap();
        assert_eq!(root1, root2);

        let tree2 = BPlusTree::open(pool2, root1, 512 / 4);
        assert_eq!(tree2.get(b"x").unwrap(), Some(b"9".to_vec()));
    }

    #[test]
    fn txn_insert_commit() {
        let (tree, _pool, _wal, _alloc, _dir) = make_tree_with_wal(4096, 1);
        let txn = tree.begin_txn(IsolationLevel::Snapshot).unwrap();
        tree.insert_txn(&txn, b"k", b"v").unwrap();
        let commit_ts = tree.commit_txn(&txn).unwrap();
        assert!(commit_ts > NULL_TS);

        // A new transaction sees the committed value.
        let txn2 = tree.begin_txn(IsolationLevel::Snapshot).unwrap();
        assert_eq!(tree.get_txn(&txn2, b"k").unwrap(), Some(b"v".to_vec()));

        // An older snapshot started before the commit does not see it.
        // (We cannot construct a read_ts in the past through the public API,
        // but we can verify the committed value is visible to autocommit.)
        assert_eq!(tree.get(b"k").unwrap(), Some(b"v".to_vec()));
    }

    #[test]
    fn txn_insert_rollback() {
        let (tree, _pool, _wal, _alloc, _dir) = make_tree_with_wal(4096, 1);
        tree.insert(b"k", b"old").unwrap();

        let txn = tree.begin_txn(IsolationLevel::Snapshot).unwrap();
        tree.insert_txn(&txn, b"k", b"new").unwrap();
        tree.rollback_txn(&txn).unwrap();

        // Autocommit readers see the old value.
        assert_eq!(tree.get(b"k").unwrap(), Some(b"old".to_vec()));

        // A new transaction also sees the old value.
        let txn2 = tree.begin_txn(IsolationLevel::Snapshot).unwrap();
        assert_eq!(tree.get_txn(&txn2, b"k").unwrap(), Some(b"old".to_vec()));
    }

    #[test]
    fn txn_update_rollback() {
        let (tree, _pool, _wal, _alloc, _dir) = make_tree_with_wal(4096, 1);
        tree.insert(b"k", b"old").unwrap();

        let txn = tree.begin_txn(IsolationLevel::Snapshot).unwrap();
        tree.insert_txn(&txn, b"k", b"new").unwrap();
        tree.rollback_txn(&txn).unwrap();

        assert_eq!(tree.get(b"k").unwrap(), Some(b"old".to_vec()));
    }

    #[test]
    fn snapshot_isolation() {
        let (tree, _pool, _wal, _alloc, _dir) = make_tree_with_wal(4096, 1);
        tree.insert(b"k", b"old").unwrap();

        let t1 = tree.begin_txn(IsolationLevel::Snapshot).unwrap();
        let t2 = tree.begin_txn(IsolationLevel::Snapshot).unwrap();

        // t2 updates and commits.
        tree.insert_txn(&t2, b"k", b"new").unwrap();
        tree.commit_txn(&t2).unwrap();

        // t1 still sees its initial snapshot.
        assert_eq!(tree.get_txn(&t1, b"k").unwrap(), Some(b"old".to_vec()));

        // A new transaction sees the committed value.
        let t3 = tree.begin_txn(IsolationLevel::Snapshot).unwrap();
        assert_eq!(tree.get_txn(&t3, b"k").unwrap(), Some(b"new".to_vec()));
    }

    #[test]
    fn read_your_writes() {
        let (tree, _pool, _wal, _alloc, _dir) = make_tree_with_wal(4096, 1);
        let txn = tree.begin_txn(IsolationLevel::Snapshot).unwrap();
        tree.insert_txn(&txn, b"k", b"uncommitted").unwrap();

        // The writing transaction sees its own uncommitted change.
        assert_eq!(
            tree.get_txn(&txn, b"k").unwrap(),
            Some(b"uncommitted".to_vec())
        );

        // Other transactions do not see it before commit.
        let other = tree.begin_txn(IsolationLevel::Snapshot).unwrap();
        assert!(tree.get_txn(&other, b"k").unwrap().is_none());
    }

    #[test]
    fn first_writer_wins() {
        let (tree, _pool, _wal, _alloc, _dir) = make_tree_with_wal(4096, 1);
        tree.insert(b"k", b"v0").unwrap();

        let t1 = tree.begin_txn(IsolationLevel::Snapshot).unwrap();
        let t2 = tree.begin_txn(IsolationLevel::Snapshot).unwrap();

        // t2 commits first.
        tree.insert_txn(&t2, b"k", b"v2").unwrap();
        tree.commit_txn(&t2).unwrap();

        // t1 started before t2 committed, so its Snapshot write conflicts.
        assert!(matches!(
            tree.insert_txn(&t1, b"k", b"v1"),
            Err(Error::Conflict)
        ));

        // ReadCommitted transactions do not conflict.
        let rc = tree.begin_txn(IsolationLevel::ReadCommitted).unwrap();
        tree.insert_txn(&rc, b"k", b"v3").unwrap();
        tree.commit_txn(&rc).unwrap();
    }

    #[test]
    #[cfg(not(miri))]
    fn txn_insert_triggers_split() {
        // Use a small page size so a transactional insert must split the leaf.
        let (tree, _pool, _wal, _alloc, _dir) = make_tree_with_wal(512, 1);
        let txn = tree.begin_txn(IsolationLevel::Snapshot).unwrap();
        let keys: Vec<String> = (0u64..40).map(|i| format!("{:08x}", i)).collect();
        for key in &keys {
            tree.insert_txn(&txn, key.as_bytes(), key.as_bytes())
                .unwrap();
        }
        tree.commit_txn(&txn).unwrap();

        tree.check_integrity().unwrap();
        for key in &keys {
            assert_eq!(
                tree.get(key.as_bytes()).unwrap(),
                Some(key.as_bytes().to_vec()),
                "key {key} missing after transactional split"
            );
        }
    }

    #[test]
    #[cfg(not(miri))]
    fn snapshot_survives_leaf_split() {
        // Fill the tree with autocommit values.
        let (tree, _pool, _wal, _alloc, _dir) = make_tree_with_wal(512, 1);
        let keys: Vec<String> = (0u64..40).map(|i| format!("{:08x}", i)).collect();
        for key in &keys {
            tree.insert(key.as_bytes(), key.as_bytes()).unwrap();
        }

        // Begin a snapshot before a transactional update that will split a leaf.
        let t1 = tree.begin_txn(IsolationLevel::Snapshot).unwrap();

        let t2 = tree.begin_txn(IsolationLevel::Snapshot).unwrap();
        let big_value = vec![b'x'; 100];
        tree.insert_txn(&t2, keys[0].as_bytes(), &big_value)
            .unwrap();
        tree.commit_txn(&t2).unwrap();

        // The older snapshot must still see the original values, even for cells
        // that were moved to a new page by the split.
        for key in &keys {
            assert_eq!(
                tree.get_txn(&t1, key.as_bytes()).unwrap(),
                Some(key.as_bytes().to_vec()),
                "t1 lost pre-split value for {key}"
            );
        }
    }

    #[test]
    #[cfg(not(miri))]
    fn snapshot_survives_leaf_merge() {
        // Fill the tree and begin a snapshot before creating MVCC versions.
        let (tree, _pool, _wal, _alloc, _dir) = make_tree_with_wal(512, 1);
        let keys: Vec<String> = (0u64..40).map(|i| format!("{:08x}", i)).collect();
        for key in &keys {
            tree.insert(key.as_bytes(), key.as_bytes()).unwrap();
        }

        let t1 = tree.begin_txn(IsolationLevel::Snapshot).unwrap();

        // Transactionally update a subset so those cells carry MVCC metadata.
        let t2 = tree.begin_txn(IsolationLevel::Snapshot).unwrap();
        let updated: Vec<String> = keys.iter().step_by(10).cloned().collect();
        for key in &updated {
            tree.insert_txn(&t2, key.as_bytes(), b"updated").unwrap();
        }
        tree.commit_txn(&t2).unwrap();

        // Autocommit-delete enough of the remaining keys to force leaf merges.
        // The survivor pages still hold the updated cells; their MVCC headers
        // must survive the merge so the older snapshot sees the original values.
        let keep: std::collections::HashSet<&String> = updated.iter().collect();
        for key in keys.iter().rev() {
            if !keep.contains(key) {
                tree.delete(key.as_bytes()).unwrap();
            }
        }

        for key in &updated {
            assert_eq!(
                tree.get_txn(&t1, key.as_bytes()).unwrap(),
                Some(key.as_bytes().to_vec()),
                "t1 lost merged MVCC value for {key}"
            );
        }
    }

    #[test]
    fn large_value_roundtrip() {
        let (tree, _pool, _wal, value_log, _alloc, _dir) =
            make_tree_with_wal_and_value_log(4096, 1);
        let big = vec![b'z'; 2048];
        tree.insert(b"k", &big).unwrap();
        assert_eq!(tree.get(b"k").unwrap(), Some(big));
        assert!(!value_log.live_refs().is_empty());
    }

    #[test]
    fn large_value_persists_after_recovery() {
        // Keep the temp dir and the expected value alive, but drop everything
        // else (including the WAL) so recovery can reopen the log directory.
        let (dir, big) = {
            let (tree, pool, wal, value_log, alloc, dir) =
                make_tree_with_wal_and_value_log(4096, 1);
            let big = vec![b'z'; 2048];
            tree.insert(b"k", &big).unwrap();
            drop(tree);
            drop(pool);
            drop(wal);
            drop(value_log);
            drop(alloc);
            (dir, big)
        };

        let disk = Arc::new(PagedFile::open(dir.path().join("pages.dat"), 4096).unwrap());
        let alloc = Arc::new(SyncMutex::new(PageAllocator::new(PageId::new(1))));
        let pool2 = Arc::new(BufferPool::new(64, 4096, disk, alloc).unwrap());
        let wal2 = Arc::new(WalLog::open(dir.path(), storage_wal::WalOptions::default()).unwrap());
        let value_log2 = Arc::new(ValueLog::open(dir.path()).unwrap());

        let fresh_root_guard = pool2.new_page().unwrap();
        let fresh_root = fresh_root_guard.page().id;
        fresh_root_guard.page().set_leaf();
        fresh_root_guard.mark_dirty();
        drop(fresh_root_guard);

        let recovery = crate::recovery::Recovery::new(pool2.clone(), wal2, fresh_root)
            .with_value_log(Arc::clone(&value_log2));
        let root = recovery.recover(NULL_LSN).unwrap();
        let tree2 = BPlusTree::open(pool2, root, 4096 / 4).with_value_log(value_log2);
        assert_eq!(tree2.get(b"k").unwrap(), Some(big));
    }

    #[test]
    fn large_value_update_releases_old_ref() {
        let (tree, _pool, _wal, value_log, _alloc, _dir) =
            make_tree_with_wal_and_value_log(4096, 1);
        let v1 = vec![b'a'; 2048];
        tree.insert(b"k", &v1).unwrap();
        let (old_offset, old_len) = value_log.live_refs()[0];

        let v2 = vec![b'b'; 3000];
        tree.insert(b"k", &v2).unwrap();
        assert_eq!(tree.get(b"k").unwrap(), Some(v2.clone()));

        let refs = value_log.live_refs();
        assert_eq!(refs.len(), 1);
        assert_ne!(refs[0], (old_offset, old_len));

        // Compact the value log and verify both the log and the tree still
        // resolve the live value.
        tree.compact_value_log().unwrap();
        let (new_offset, new_len) = value_log.live_refs()[0];
        assert_eq!(value_log.read(new_offset, new_len).unwrap(), v2);
        assert_eq!(tree.get(b"k").unwrap(), Some(v2));
        assert!(value_log.read(old_offset, old_len).is_err());
    }

    #[test]
    fn large_value_gc_reclaims_dead_value() {
        let (tree, _pool, _wal, value_log, _alloc, _dir) =
            make_tree_with_wal_and_value_log(4096, 1);
        let big = vec![b'x'; 2048];
        tree.insert(b"k", &big).unwrap();
        let (offset, len) = value_log.live_refs()[0];

        tree.delete(b"k").unwrap();
        assert!(tree.get(b"k").unwrap().is_none());
        assert!(value_log.live_refs().is_empty());

        tree.compact_value_log().unwrap();
        assert!(tree.get(b"k").unwrap().is_none());
        assert!(value_log.read(offset, len).is_err());
    }

    #[test]
    fn large_value_gc_updates_many_leaf_refs() {
        let (tree, _pool, _wal, _value_log, _alloc, _dir) =
            make_tree_with_wal_and_value_log(4096, 1);
        let mut values: Vec<Vec<u8>> = Vec::new();
        for i in 0u8..5 {
            let v = vec![i; 2048];
            tree.insert(&[i], &v).unwrap();
            values.push(v);
        }

        // Overwrite half the keys so their original values become dead.
        for i in 0u8..2 {
            let v = vec![0x80 + i; 2500];
            tree.insert(&[i], &v).unwrap();
            values[i as usize] = v;
        }

        tree.compact_value_log().unwrap();

        for i in 0u8..5 {
            assert_eq!(tree.get(&[i]).unwrap(), Some(values[i as usize].clone()));
        }
    }

    #[test]
    fn txn_large_value_commit_and_rollback() {
        let (tree, _pool, _wal, value_log, _alloc, _dir) =
            make_tree_with_wal_and_value_log(4096, 1);
        let committed_value = vec![b'c'; 2048];
        let rolled_back_value = vec![b'r'; 3000];

        let t1 = tree.begin_txn(IsolationLevel::Snapshot).unwrap();
        tree.insert_txn(&t1, b"k", &committed_value).unwrap();
        tree.commit_txn(&t1).unwrap();
        assert_eq!(tree.get(b"k").unwrap(), Some(committed_value.clone()));

        let t2 = tree.begin_txn(IsolationLevel::Snapshot).unwrap();
        tree.insert_txn(&t2, b"k", &rolled_back_value).unwrap();
        tree.rollback_txn(&t2).unwrap();
        assert_eq!(tree.get(b"k").unwrap(), Some(committed_value));

        // The rolled-back value-log entry must have been released.
        assert_eq!(value_log.live_refs().len(), 1);
    }

    #[test]
    fn snapshot_pin_root_tracks_active_snapshots() {
        let (tree, _dir) = make_tree(512, 1);
        let root = tree.root_page_id();
        tree.pin_root(root);
        tree.pin_root(root);
        tree.unpin_root(root);
        tree.unpin_root(root);
        // Should not panic and the root should be unpinned.
        tree.unpin_root(root);
    }

    #[test]
    fn snapshot_reachable_pages_accounts_for_all_nodes() {
        let (tree, _dir) = make_tree(512, 1);
        for i in 0u64..30 {
            let key = format!("{:08x}", i);
            tree.insert(key.as_bytes(), b"v").unwrap();
        }
        let reachable = tree.reachable_pages(tree.root_page_id()).unwrap();
        assert!(!reachable.is_empty());
        // The root must always be reachable.
        assert!(reachable.contains(&tree.root_page_id()));
        tree.check_integrity().unwrap();
    }

    #[test]
    fn snapshot_compact_honors_pinned_roots() {
        let (tree, _dir) = make_tree(512, 1);

        // Create enough data to cause splits.
        for i in 0u64..40 {
            let key = format!("{:08x}", i);
            tree.insert(key.as_bytes(), b"v").unwrap();
        }
        let snapshot_root = tree.root_page_id();
        tree.pin_root(snapshot_root);

        // Delete most keys to trigger merges and retire pages.
        for i in 0u64..40 {
            if i % 5 != 0 {
                let key = format!("{:08x}", i);
                tree.delete(key.as_bytes()).unwrap();
            }
        }
        tree.check_integrity().unwrap();

        // Compact while the old root is pinned.  Pages reachable from the
        // pinned snapshot must survive.
        tree.compact().unwrap();
        tree.check_integrity().unwrap();

        // The remaining keys must still be readable.
        for i in 0u64..40 {
            if i % 5 == 0 {
                let key = format!("{:08x}", i);
                assert_eq!(tree.get(key.as_bytes()).unwrap(), Some(b"v".to_vec()));
            }
        }

        tree.unpin_root(snapshot_root);
        tree.compact().unwrap();
        tree.check_integrity().unwrap();
    }

    #[test]
    fn writer_pins_leaf_frame_during_optimistic_traversal() {
        let (tree, _dir) = make_tree(512, 1);
        tree.insert(b"k", b"v").unwrap();

        let target = tree.optimistic_path_to_leaf(b"k").unwrap().unwrap();
        let leaf_id = target.leaf_arc.id;
        let pin_count = tree.pool().frame_pin_count(leaf_id).unwrap();
        assert!(
            pin_count > 0,
            "leaf frame must be pinned while the optimistic path is held"
        );

        drop(target);
        let pin_count = tree.pool().frame_pin_count(leaf_id).unwrap();
        assert_eq!(pin_count, 0, "leaf pin must be released when path is dropped");
    }

    #[test]
    fn rollback_txn_restores_previous_value() {
        let (tree, _pool, _wal, _alloc, _dir) = make_tree_with_wal(4096, 1);
        tree.insert(b"k", b"old").unwrap();

        let txn = tree.begin_txn(IsolationLevel::Snapshot).unwrap();
        tree.insert_txn(&txn, b"k", b"new").unwrap();
        tree.rollback_txn(&txn).unwrap();

        assert_eq!(tree.get(b"k").unwrap(), Some(b"old".to_vec()));
    }

    #[test]
    #[cfg(not(miri))]
    fn rollback_concurrent_with_readers_and_writers() {
        // Exercise rollback while other threads read and write disjoint keys.
        // The rollbacker holds per-page OLC write latches, so this must not
        // deadlock or corrupt the tree structure.
        let (tree, _pool, _wal, _alloc, _dir) = make_tree_with_wal(512, 1);
        let tree = Arc::new(tree);
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let mut handles = Vec::new();

        let t = Arc::clone(&tree);
        let b = Arc::clone(&barrier);
        handles.push(std::thread::spawn(move || {
            b.wait();
            for i in 0..100u64 {
                let key = format!("w{:04}", i);
                let value = format!("writer-{i}");
                t.insert(key.as_bytes(), value.as_bytes()).unwrap();
            }
        }));

        let t = Arc::clone(&tree);
        let b = Arc::clone(&barrier);
        handles.push(std::thread::spawn(move || {
            b.wait();
            for i in 0..100u64 {
                let key = format!("w{:04}", i);
                let _ = t.get(key.as_bytes());
            }
        }));

        let t = Arc::clone(&tree);
        let b = Arc::clone(&barrier);
        handles.push(std::thread::spawn(move || {
            b.wait();
            for i in 0..50 {
                let txn = t.begin_txn(IsolationLevel::Snapshot).unwrap();
                let key = format!("r{:04}", i);
                t.insert_txn(&txn, key.as_bytes(), format!("rollback-{i}").as_bytes())
                    .unwrap();
                t.rollback_txn(&txn).unwrap();
            }
        }));

        for h in handles {
            h.join().unwrap();
        }

        tree.check_integrity().unwrap();
    }
}
