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

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::{BoundKind, Error, Result};
use crate::v2::buffer::{BufferPool, PageGuard};
use crate::v2::page::{NULL_PAGE_ID, PageId, WriteGuard};
use crate::v2::slot::{OwnedValue, Slot, ValueKind};

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

/// Pinned path captured during an optimistic traversal.  The leaf is kept
/// resident via `leaf_guard`; `leaf_arc` provides cheap shared access for
/// locking and validation.
struct OptimisticPath {
    leaf_guard: PageGuard,
    leaf_arc: Arc<crate::v2::page::Page>,
    /// Root page id observed at the start of the traversal.
    root_id: PageId,
    path: Vec<(PageGuard, Arc<crate::v2::page::Page>, u64)>,
}

impl OptimisticPath {
    /// True if every captured ancestor version is still current and each
    /// ancestor still points to the next page on the path for `key`.
    fn path_valid(&self, key: &[u8]) -> bool {
        Self::path_valid_for(&self.path, key, self.leaf_arc.id)
    }

    fn path_valid_for(
        path: &[(PageGuard, Arc<crate::v2::page::Page>, u64)],
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
    /// Maximum inline value size; larger values are rejected until the value
    /// log is implemented in Phase 7.
    inline_threshold: usize,
    /// Minimum number of live cells a page must retain; below this the page
    /// tries to redistribute or merge.
    min_cells: usize,
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
            root_page_id: AtomicU64::new(root_id),
            inline_threshold,
            min_cells: DEFAULT_MIN_CELLS,
        })
    }

    /// Create a tree with an explicit minimum cell count, useful in tests with
    /// very small pages.
    #[cfg(test)]
    fn with_min_cells(
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
            root_page_id: AtomicU64::new(root_id),
            inline_threshold,
            min_cells,
        })
    }

    /// Return the current root page id.
    pub fn root_page_id(&self) -> PageId {
        self.root_page_id.load(Ordering::SeqCst)
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
        if key.is_empty() {
            return Err(Error::InvalidArgument(
                "empty keys are not supported".into(),
            ));
        }
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
        if self.root_page_id.load(Ordering::Acquire) != leaf.root_id {
            return Ok(None);
        }

        let page = leaf.guard.page();
        let opt = match page.optimistic() {
            Some(o) => o,
            None => return Ok(None),
        };

        let result = opt.read(|p| {
            p.get(key).map(|opt_cell| {
                opt_cell.and_then(|c| match c.value {
                    ValueKind::Inline(v) => Some(v.to_vec()),
                    ValueKind::Tombstone => None,
                    ValueKind::ValueLog { .. } => {
                        // Phase 7 will resolve value-log references.
                        None
                    }
                })
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
        let root_id = self.root_page_id.load(Ordering::Acquire);
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
                return Ok(Some(OptimisticLeaf { guard, root_id, path }));
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
        if key.is_empty() {
            return Err(Error::InvalidArgument(
                "empty keys are not supported".into(),
            ));
        }
        if value.len() > self.inline_threshold {
            return Err(Error::OutOfBounds {
                kind: BoundKind::Value,
                limit: self.inline_threshold,
                got: value.len(),
            });
        }
        let value_kind = ValueKind::Inline(value);
        loop {
            match self.lock_coupled_insert(key, &value_kind)? {
                Some(()) => {
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

    fn lock_coupled_insert(&self, key: &[u8], value: &ValueKind<'_>) -> Result<Option<()>> {
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
        if self.root_page_id.load(Ordering::Acquire) != target.root_id {
            return Ok(None);
        }
        // Validate the captured root-to-leaf path.
        if !target.path_valid(key) {
            return Ok(None);
        }

match leaf_write.page().insert(key, value) {
            Ok(_) => {
                self.pool.mark_dirty(leaf_arc.id)?;
                return Ok(Some(()));
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
        let mut locked_arcs: Vec<Arc<crate::v2::page::Page>> =
            Vec::with_capacity(path.len() + 1);
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
        if key >= separator.as_slice() {
            right_write.page().insert(key, value)?;
        } else {
            locked[0].page().insert(key, value)?;
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

            match parent_write
                .page()
                .insert(&split.separator, &ValueKind::Inline(&child_bytes))
            {
                Ok(_) => {
                    self.pool.mark_dirty(parent_id)?;
                    return Ok(Some(()));
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

                    let promoted =
                        split_internal(parent_write.page(), new_right_write.page())?;
                    if split.separator.as_slice() > promoted.as_slice() {
                        new_right_write
                            .page()
                            .insert(&split.separator, &ValueKind::Inline(&child_bytes))?;
                    } else {
                        parent_write
                            .page()
                            .insert(&split.separator, &ValueKind::Inline(&child_bytes))?;
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
        let root_write = locked.last().expect("locked path contains at least the leaf");
        let root_id = root_write.page().id;
        let child_bytes = encode_page_id(split.right_page_id);

        match root_write
            .page()
            .insert(&split.separator, &ValueKind::Inline(&child_bytes))
        {
            Ok(_) => {
                self.pool.mark_dirty(root_id)?;
                return Ok(Some(()));
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
        if split.separator.as_slice() > promoted.as_slice() {
            new_right_write
                .page()
                .insert(&split.separator, &ValueKind::Inline(&child_bytes))?;
        } else {
            root_write
                .page()
                .insert(&split.separator, &ValueKind::Inline(&child_bytes))?;
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
        new_root_write.page().set_leftmost_child(root_id);
        let bytes = encode_page_id(new_right_arc.id);
        new_root_write
            .page()
            .insert(&promoted, &ValueKind::Inline(&bytes))?;
        self.pool.mark_dirty(new_root_arc.id)?;

        self.root_page_id.store(new_root_arc.id, Ordering::SeqCst);
        Ok(Some(()))
    }

    fn split_root_leaf_locked(
        &self,
        root_id: PageId,
        root_write: WriteGuard<'_>,
        key: &[u8],
        value: &ValueKind<'_>,
    ) -> Result<Option<()>> {
        let right_guard = self.pool.new_page()?;
        let right_arc = right_guard.page_arc();
        let right_write = match right_arc.try_write() {
            Some(w) => w,
            None => return Ok(None),
        };
        right_write.page().set_leaf();

        let separator = split_leaf(root_write.page(), right_write.page())?;
        if key >= separator.as_slice() {
            right_write.page().insert(key, value)?;
        } else {
            root_write.page().insert(key, value)?;
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
        new_root_write.page().set_leftmost_child(root_id);
        let bytes = encode_page_id(right_arc.id);
        new_root_write
            .page()
            .insert(&separator, &ValueKind::Inline(&bytes))?;
        self.pool.mark_dirty(new_root_arc.id)?;

        self.root_page_id.store(new_root_arc.id, Ordering::SeqCst);
        Ok(Some(()))
    }

    /// Optimistically descend from the current root to the leaf that should
    /// contain `key`, keeping a pinned path of ancestors with their latch
    /// versions.  Returns `None` if a page changes during descent.
    fn optimistic_path_to_leaf(&self, key: &[u8]) -> Result<Option<OptimisticPath>> {
        let root_id = self.root_page_id.load(Ordering::Acquire);
        let mut path: Vec<(PageGuard, Arc<crate::v2::page::Page>, u64)> = Vec::new();
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
                    leaf_guard: guard,
                    leaf_arc: arc,
                    root_id,
                    path,
                }));
            }

            let child_id = child_for_key(&arc, key)?;
            if arc.latch_word() != version {
                return Ok(None);
            }

            path.push((guard, arc, version));
            current_id = child_id;
        }
    }

    fn link_siblings_after_split_locked(
        &self,
        left_id: PageId,
        left_page: &crate::v2::page::Page,
        right_id: PageId,
        right_page: &crate::v2::page::Page,
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
        if key.is_empty() {
            return Err(Error::InvalidArgument(
                "empty keys are not supported".into(),
            ));
        }

        loop {
            match self.lock_coupled_delete(key)? {
                Some(existed) => return Ok(existed),
                None => continue,
            }
        }
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
        if self.root_page_id.load(Ordering::Acquire) != target.root_id {
            return Ok(None);
        }
        // Validate the captured root-to-leaf path.
        if !target.path_valid(key) {
            return Ok(None);
        }

        let leaf_id = leaf_arc.id;
        let existed = leaf_write.page().delete(key)?;
        if !existed {
            return Ok(Some(false));
        }
        self.pool.mark_dirty(leaf_id)?;

        let live = leaf_write.page().live_count()?;
        if live >= self.min_cells || leaf_id == self.root_page_id() {
            return Ok(Some(true));
        }

        // Leaf underflow.  The root is allowed to become empty; any other leaf
        // must redistribute or merge with a sibling.  Lock the entire path so
        // that structure modifications are atomic with respect to concurrent
        // readers and writers.
        if target.path.is_empty() {
            return Ok(Some(true));
        }
        match self.handle_leaf_underflow_olc(target, leaf_write)? {
            None => Ok(None),
            Some(dead) => {
                // All tree latches are now released.  Free pages that became
                // unreachable during the merge.
                for id in dead {
                    self.pool.free_page(id)?;
                }
                Ok(Some(true))
            }
        }
    }

    // ------------------------------------------------------------------
    // OLC underflow handling
    // ------------------------------------------------------------------

    /// Lock the entire root-to-leaf path captured in `target` and handle a leaf
    /// underflow.  All latches are acquired top-down and validated against the
    /// versions observed during the optimistic traversal.  Dead page ids are
    /// returned so the caller can free them after releasing all latches.
    fn handle_leaf_underflow_olc(
        &self,
        target: OptimisticPath,
        leaf_write: WriteGuard<'_>,
    ) -> Result<Option<Vec<PageId>>> {
        let mut locked_arcs: Vec<Arc<crate::v2::page::Page>> =
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

        let mut dead: Vec<PageId> = Vec::new();
        self.handle_leaf_underflow_locked(&mut locked, &mut dead)?;
        Ok(Some(dead))
    }

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
        let pos = child_position(locked[1].page(), leaf_id)?.ok_or_else(|| {
            Error::Corruption("underflowed leaf not found in parent".into())
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
        if let Some(right_id) = right_sibling {
            self.merge_leaf_with_sibling(locked, leaf_id, right_id, true, dead)?;
        } else if let Some(left_id) = left_sibling {
            self.merge_leaf_with_sibling(locked, leaf_id, left_id, false, dead)?;
        } else {
            return Err(Error::Corruption(format!(
                "leaf page {leaf_id} has no sibling to merge with"
            )));
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
            redistribute_leaf_right_to_left(leaf_write.page(), sibling_write.page())?;
            self.pool.mark_dirty(leaf_id)?;
            self.pool.mark_dirty(sibling_id)?;
            let new_sep = sibling_write.page().get_by_slot(0)?.key.to_vec();
            let pos = child_position(parent_write.page(), leaf_id)?.ok_or_else(|| {
                Error::Corruption("underflowed leaf not found in parent".into())
            })?;
            update_separator(parent_write.page(), pos + 1, &new_sep)?;
        } else {
            redistribute_leaf_left_to_right(sibling_write.page(), leaf_write.page())?;
            self.pool.mark_dirty(leaf_id)?;
            self.pool.mark_dirty(sibling_id)?;
            let new_sep = leaf_write.page().get_by_slot(0)?.key.to_vec();
            let pos = child_position(parent_write.page(), leaf_id)?.ok_or_else(|| {
                Error::Corruption("underflowed leaf not found in parent".into())
            })?;
            update_separator(parent_write.page(), pos, &new_sep)?;
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
            None => return Ok(None),
        };

        let parent_write = &locked[1];
        let parent_id = parent_write.page().id;
        let pos = child_position(parent_write.page(), leaf_id)?.ok_or_else(|| {
            Error::Corruption("underflowed leaf not found in parent".into())
        })?;

        if sibling_is_right {
            merge_leaf_right_into_left(
                locked[0].page(),
                sibling_write.page(),
                &self.pool,
            )?;
            self.pool.mark_dirty(leaf_id)?;
            remove_child_at(parent_write.page(), pos)?;
            dead.push(sibling_id);
        } else {
            merge_leaf_right_into_left(
                sibling_write.page(),
                locked[0].page(),
                &self.pool,
            )?;
            self.pool.mark_dirty(sibling_id)?;
            remove_child_at(parent_write.page(), pos - 1)?;
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
                    // and install the new root.  We cannot free it until the
                    // write latch is released.
                    self.root_page_id.store(new_root_id, Ordering::SeqCst);
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
        if let Some(right_id) = right_sibling {
            self.merge_internal_with_sibling(locked, node_id, right_id, true, dead)?;
        } else if let Some(left_id) = left_sibling {
            self.merge_internal_with_sibling(locked, node_id, left_id, false, dead)?;
        } else {
            return Err(Error::Corruption(format!(
                "internal page {node_id} has no sibling to merge with"
            )));
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
            let new_sep = redistribute_internal_right_to_left(
                node_write.page(),
                sibling_write.page(),
                parent_write.page(),
                pos,
            )?;
            self.pool.mark_dirty(node_id)?;
            self.pool.mark_dirty(sibling_id)?;
            update_separator(parent_write.page(), pos + 1, &new_sep)?;
        } else {
            let new_sep = redistribute_internal_left_to_right(
                sibling_write.page(),
                node_write.page(),
                parent_write.page(),
                pos - 1,
            )?;
            self.pool.mark_dirty(node_id)?;
            self.pool.mark_dirty(sibling_id)?;
            update_separator(parent_write.page(), pos, &new_sep)?;
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
            None => return Ok(None),
        };

        let parent_write = &locked[1];
        let parent_id = parent_write.page().id;
        let pos = child_position(parent_write.page(), node_id)?.ok_or_else(|| {
            Error::Corruption("underflowed internal node not found in parent".into())
        })?;

        if sibling_is_right {
            let separator = parent_write.page().get_by_slot(pos)?.key.to_vec();
            merge_internal_right_into_left(
                locked[0].page(),
                sibling_write.page(),
                &separator,
            )?;
            self.pool.mark_dirty(node_id)?;
            remove_child_at(parent_write.page(), pos)?;
            dead.push(sibling_id);
        } else {
            let separator = parent_write.page().get_by_slot(pos - 1)?.key.to_vec();
            merge_internal_right_into_left(
                sibling_write.page(),
                locked[0].page(),
                &separator,
            )?;
            self.pool.mark_dirty(sibling_id)?;
            remove_child_at(parent_write.page(), pos - 1)?;
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
        self.check_node(root, &mut seen, &mut leaf_prev, true, None, None)?;
        Ok(())
    }

    // ------------------------------------------------------------------
    // Integrity check
    // ------------------------------------------------------------------

    fn check_node(
        &self,
        page: &crate::v2::page::Page,
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

            let mut last: Option<&[u8]> = None;
            for idx in 0..page.slot_count()? {
                if page.read_slot(idx)?.is_deleted() {
                    continue;
                }
                let cell = page.get_by_slot(idx)?;
                if let Some(low) = low
                    && cell.key < low
                {
                    return Err(Error::Corruption(format!(
                        "leaf page {} key {:?} below low bound {:?}",
                        page.id, cell.key, low
                    )));
                }
                if let Some(high) = high
                    && cell.key >= high
                {
                    return Err(Error::Corruption(format!(
                        "leaf page {} key {:?} at or above high bound {:?}",
                        page.id, cell.key, high
                    )));
                }
                if let Some(last) = last
                    && cell.key <= last
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

        let mut last_key: Option<&[u8]> = None;
        for idx in 0..page.slot_count()? {
            if page.read_slot(idx)?.is_deleted() {
                continue;
            }
            let cell = page.get_by_slot(idx)?;
            let child_id = decode_page_id(&cell.value)?;
            if !seen.insert(child_id) {
                return Err(Error::Corruption(format!(
                    "page {child_id} already visited (cycle) from internal page {}",
                    page.id
                )));
            }
            if let Some(last) = last_key
                && cell.key <= last
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
            entries.push((cell.key.to_vec(), decode_page_id(&cell.value)?));
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

    // ------------------------------------------------------------------
    // Merge / redistribute path
    // ------------------------------------------------------------------

    fn handle_leaf_underflow(
        &self,
        mut stack: Vec<PageGuard>,
        leaf_guard: PageGuard,
    ) -> Result<()> {
        let leaf_id = leaf_guard.page().id;
        let parent_guard = stack
            .pop()
            .ok_or_else(|| Error::Corruption("leaf underflow with no parent".into()))?;
        let parent_id = parent_guard.page().id;
        let pos = child_position(parent_guard.page(), leaf_id)?
            .ok_or_else(|| Error::Corruption("underflowed leaf not found in parent".into()))?;
        let right_sibling = right_child_at(parent_guard.page(), pos)?;
        let left_sibling = if pos > 0 {
            Some(child_at(parent_guard.page(), pos - 1)?)
        } else {
            None
        };

        // Redistribute from right sibling.
        if let Some(right_id) = right_sibling {
            let right_guard = self.pool.fetch_or_read(right_id)?;
            if can_give_cell(right_guard.page(), self.min_cells) {
                redistribute_leaf_right_to_left(leaf_guard.page(), right_guard.page())?;
                leaf_guard.mark_dirty();
                right_guard.mark_dirty();
                let new_sep = right_guard.page().get_by_slot(0)?.key.to_vec();
                // Separator between leaf (pos) and right sibling (pos+1) is cell[pos].
                update_separator(parent_guard.page(), pos + 1, &new_sep)?;
                parent_guard.mark_dirty();
                return Ok(());
            }
        }

        // Redistribute from left sibling.
        if let Some(left_id) = left_sibling {
            let left_guard = self.pool.fetch_or_read(left_id)?;
            if can_give_cell(left_guard.page(), self.min_cells) {
                redistribute_leaf_left_to_right(left_guard.page(), leaf_guard.page())?;
                leaf_guard.mark_dirty();
                left_guard.mark_dirty();
                let new_sep = leaf_guard.page().get_by_slot(0)?.key.to_vec();
                // Separator between left sibling (pos-1) and leaf (pos) is cell[pos-1].
                update_separator(parent_guard.page(), pos, &new_sep)?;
                parent_guard.mark_dirty();
                return Ok(());
            }
        }

        // Merge with a sibling.
        if let Some(right_id) = right_sibling {
            let right_guard = self.pool.fetch_or_read(right_id)?;
            merge_leaf_right_into_left(leaf_guard.page(), right_guard.page(), &self.pool)?;
            leaf_guard.mark_dirty();
            remove_child_at(parent_guard.page(), pos)?;
            parent_guard.mark_dirty();
            // Drop the right guard before freeing its frame, otherwise free_page
            // would deadlock on the frame lock held by the guard.
            drop(right_guard);
            self.pool.free_page(right_id)?;
            self.propagate_internal_underflow(stack, parent_guard, parent_id, 0)?;
        } else if let Some(left_id) = left_sibling {
            let left_guard = self.pool.fetch_or_read(left_id)?;
            merge_leaf_right_into_left(left_guard.page(), leaf_guard.page(), &self.pool)?;
            left_guard.mark_dirty();
            remove_child_at(parent_guard.page(), pos - 1)?;
            parent_guard.mark_dirty();
            // Drop the leaf guard before freeing its frame.
            drop(leaf_guard);
            self.pool.free_page(leaf_id)?;
            self.propagate_internal_underflow(stack, parent_guard, parent_id, 0)?;
        } else {
            return Err(Error::Corruption(format!(
                "leaf page {leaf_id} has no sibling to merge with"
            )));
        }

        Ok(())
    }

    fn propagate_internal_underflow(
        &self,
        mut stack: Vec<PageGuard>,
        node_guard: PageGuard,
        node_id: PageId,
        depth: usize,
    ) -> Result<()> {
        if depth > 64 {
            return Err(Error::Corruption(format!(
                "propagate_internal_underflow exceeded depth limit at node {node_id}"
            )));
        }
        let live = node_guard.page().live_count()?;
        if live >= self.min_cells {
            return Ok(());
        }

        let root_id = self.root_page_id();
        if node_id == root_id {
            if node_guard.page().is_internal() && live == 0 {
                let new_root_id = node_guard.page().leftmost_child()?;
                if new_root_id != NULL_PAGE_ID {
                    // Drop the guard before freeing the old root frame to avoid
                    // deadlocking on the frame lock.
                    drop(node_guard);
                    self.root_page_id.store(new_root_id, Ordering::SeqCst);
                    self.pool.free_page(node_id)?;
                }
            }
            return Ok(());
        }

        let parent_guard = stack
            .pop()
            .ok_or_else(|| Error::Corruption("internal underflow with no parent".into()))?;
        let parent_id = parent_guard.page().id;
        let pos = child_position(parent_guard.page(), node_id)?.ok_or_else(|| {
            Error::Corruption("underflowed internal node not found in parent".into())
        })?;
        let right_sibling = right_child_at(parent_guard.page(), pos)?;
        let left_sibling = if pos > 0 {
            Some(child_at(parent_guard.page(), pos - 1)?)
        } else {
            None
        };

        // Redistribute from right sibling.
        if let Some(right_id) = right_sibling {
            let right_guard = self.pool.fetch_or_read(right_id)?;
            if can_give_cell(right_guard.page(), self.min_cells) {
                let new_sep = redistribute_internal_right_to_left(
                    node_guard.page(),
                    right_guard.page(),
                    parent_guard.page(),
                    pos,
                )?;
                node_guard.mark_dirty();
                right_guard.mark_dirty();
                // The separator between node (pos) and right sibling (pos+1) is
                // stored in cell[pos].
                update_separator(parent_guard.page(), pos + 1, &new_sep)?;
                parent_guard.mark_dirty();
                return Ok(());
            }
        }

        // Redistribute from left sibling.
        if let Some(left_id) = left_sibling {
            let left_guard = self.pool.fetch_or_read(left_id)?;
            if can_give_cell(left_guard.page(), self.min_cells) {
                let new_sep = redistribute_internal_left_to_right(
                    left_guard.page(),
                    node_guard.page(),
                    parent_guard.page(),
                    pos - 1,
                )?;
                left_guard.mark_dirty();
                node_guard.mark_dirty();
                update_separator(parent_guard.page(), pos, &new_sep)?;
                parent_guard.mark_dirty();
                return Ok(());
            }
        }

        // Merge with a sibling.
        if let Some(right_id) = right_sibling {
            let right_guard = self.pool.fetch_or_read(right_id)?;
            // Separator between node (pos) and right sibling (pos+1) is cell[pos].
            let separator = parent_guard.page().get_by_slot(pos)?.key.to_vec();
            merge_internal_right_into_left(
                node_guard.page(),
                right_guard.page(),
                &separator,
            )?;
            node_guard.mark_dirty();
            remove_child_at(parent_guard.page(), pos)?;
            parent_guard.mark_dirty();
            drop(right_guard);
            self.pool.free_page(right_id)?;
            self.propagate_internal_underflow(stack, parent_guard, parent_id, depth + 1)?;
        } else if let Some(left_id) = left_sibling {
            let left_guard = self.pool.fetch_or_read(left_id)?;
            // Separator between left sibling (pos-1) and node (pos) is cell[pos-1].
            let separator = parent_guard.page().get_by_slot(pos - 1)?.key.to_vec();
            merge_internal_right_into_left(
                left_guard.page(),
                node_guard.page(),
                &separator,
            )?;
            left_guard.mark_dirty();
            remove_child_at(parent_guard.page(), pos - 1)?;
            parent_guard.mark_dirty();
            drop(node_guard);
            self.pool.free_page(node_id)?;
            self.propagate_internal_underflow(stack, parent_guard, parent_id, depth + 1)?;
        }

        Ok(())
    }
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

fn owned_value(value: &ValueKind<'_>) -> Result<OwnedValue> {
    match value {
        ValueKind::Inline(v) => Ok(OwnedValue::Inline(v.to_vec())),
        ValueKind::ValueLog { offset, len } => Ok(OwnedValue::ValueLog {
            offset: *offset,
            len: *len,
        }),
        ValueKind::Tombstone => Ok(OwnedValue::Tombstone),
    }
}

fn child_for_key(page: &crate::v2::page::Page, key: &[u8]) -> Result<PageId> {
    let count = page.slot_count()?;
    let mut last_live_child: Option<PageId> = None;
    for idx in 0..count {
        if page.read_slot(idx)?.is_deleted() {
            continue;
        }
        let cell = page.get_by_slot(idx)?;
        if key < cell.key {
            return if let Some(child) = last_live_child {
                Ok(child)
            } else {
                page.leftmost_child()
            };
        }
        last_live_child = Some(decode_page_id(&cell.value)?);
    }
    if let Some(child) = last_live_child {
        Ok(child)
    } else {
        page.leftmost_child()
    }
}

fn child_position(page: &crate::v2::page::Page, child_id: PageId) -> Result<Option<usize>> {
    if page.leftmost_child()? == child_id {
        return Ok(Some(0));
    }
    let count = page.slot_count()?;
    for idx in 0..count {
        if page.read_slot(idx)?.is_deleted() {
            continue;
        }
        let cell = page.get_by_slot(idx)?;
        if decode_page_id(&cell.value)? == child_id {
            return Ok(Some(idx + 1));
        }
    }
    Ok(None)
}

fn child_at(page: &crate::v2::page::Page, pos: usize) -> Result<PageId> {
    if pos == 0 {
        page.leftmost_child()
    } else {
        decode_page_id(&page.get_by_slot(pos - 1)?.value)
    }
}

fn right_child_at(page: &crate::v2::page::Page, pos: usize) -> Result<Option<PageId>> {
    let count = page.slot_count()?;
    if pos < count {
        Ok(Some(child_at(page, pos + 1)?))
    } else {
        Ok(None)
    }
}

fn can_give_cell(page: &crate::v2::page::Page, min_cells: usize) -> bool {
    page.live_count().is_ok_and(|n| n > min_cells)
}

fn split_leaf(
    left: &crate::v2::page::Page,
    right: &crate::v2::page::Page,
) -> Result<Vec<u8>> {
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

    let mut moved: Vec<(Vec<u8>, OwnedValue)> = Vec::new();
    for idx in mid..count {
        let cell = left.get_by_slot(idx)?;
        moved.push((cell.key.to_vec(), owned_value(&cell.value)?));
    }

    for idx in (mid..count).rev() {
        left.write_slot(idx, Slot::deleted());
    }
    left.compact()?;

    for (key, owned) in moved {
        right.insert(&key, &owned.as_value_kind())?;
    }

    if right.slot_count()? == 0 {
        return Err(Error::Corruption(
            "split produced an empty right page".into(),
        ));
    }
    let separator = right.get_by_slot(0)?.key.to_vec();
    Ok(separator)
}

fn split_internal(
    left: &crate::v2::page::Page,
    right: &crate::v2::page::Page,
) -> Result<Vec<u8>> {
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
    let promoted_key = promoted_cell.key.to_vec();
    let right_leftmost = decode_page_id(&promoted_cell.value)?;

    let mut moved: Vec<(Vec<u8>, PageId)> = Vec::new();
    for idx in (mid + 1)..count {
        let cell = left.get_by_slot(idx)?;
        moved.push((cell.key.to_vec(), decode_page_id(&cell.value)?));
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

fn redistribute_leaf_right_to_left(
    left: &crate::v2::page::Page,
    right: &crate::v2::page::Page,
) -> Result<()> {
    right.compact()?;
    let cell = right.get_by_slot(0)?;
    let key = cell.key.to_vec();
    let owned = owned_value(&cell.value)?;
    right.delete(&key)?;
    right.compact()?;
    left.insert(&key, &owned.as_value_kind())?;
    Ok(())
}

fn redistribute_leaf_left_to_right(
    left: &crate::v2::page::Page,
    right: &crate::v2::page::Page,
) -> Result<()> {
    left.compact()?;
    right.compact()?;
    let last_idx = left.slot_count()? - 1;
    let cell = left.get_by_slot(last_idx)?;
    let key = cell.key.to_vec();
    let owned = owned_value(&cell.value)?;
    left.write_slot(last_idx, Slot::deleted());
    left.compact()?;
    right.insert(&key, &owned.as_value_kind())?;
    Ok(())
}

fn merge_leaf_right_into_left(
    left: &crate::v2::page::Page,
    right: &crate::v2::page::Page,
    pool: &Arc<BufferPool>,
) -> Result<()> {
    left.compact()?;
    right.compact()?;
    let count = right.slot_count()?;
    for idx in 0..count {
        let cell = right.get_by_slot(idx)?;
        let owned = owned_value(&cell.value)?;
        left.insert(cell.key, &owned.as_value_kind())?;
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

fn redistribute_internal_right_to_left(
    left: &crate::v2::page::Page,
    right: &crate::v2::page::Page,
    parent: &crate::v2::page::Page,
    left_pos: usize,
) -> Result<Vec<u8>> {
    right.compact()?;
    left.compact()?;

    // Parent separator between left and right becomes the new rightmost cell
    // of left, with right's old leftmost child as its right child.
    let separator = parent.get_by_slot(left_pos - 1)?.key.to_vec();
    let old_right_leftmost = right.leftmost_child()?;
    let bytes = encode_page_id(old_right_leftmost);
    left.insert(&separator, &ValueKind::Inline(&bytes))?;

    // Move the first cell of right to become the new leftmost child of right,
    // and update the parent separator to that cell's key.
    let first = right.get_by_slot(0)?;
    let new_separator = first.key.to_vec();
    let new_right_leftmost = decode_page_id(&first.value)?;
    right.write_slot(0, Slot::deleted());
    right.compact()?;
    right.set_leftmost_child(new_right_leftmost);

    Ok(new_separator)
}

fn redistribute_internal_left_to_right(
    left: &crate::v2::page::Page,
    right: &crate::v2::page::Page,
    parent: &crate::v2::page::Page,
    left_pos: usize,
) -> Result<Vec<u8>> {
    left.compact()?;
    right.compact()?;

    // Parent separator becomes a new cell in right whose right child is right's
    // old leftmost child.
    let separator = parent.get_by_slot(left_pos)?.key.to_vec();
    let old_right_leftmost = right.leftmost_child()?;
    let bytes = encode_page_id(old_right_leftmost);
    right.insert(&separator, &ValueKind::Inline(&bytes))?;

    // Move the last cell of left to become the new leftmost child of right,
    // and update the parent separator to that cell's key.
    let last_idx = left.slot_count()? - 1;
    let last = left.get_by_slot(last_idx)?;
    let new_separator = last.key.to_vec();
    let new_right_leftmost = decode_page_id(&last.value)?;
    left.write_slot(last_idx, Slot::deleted());
    left.compact()?;
    right.set_leftmost_child(new_right_leftmost);

    Ok(new_separator)
}

fn merge_internal_right_into_left(
    left: &crate::v2::page::Page,
    right: &crate::v2::page::Page,
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
        let child_id = decode_page_id(&cell.value)?;
        let bytes = encode_page_id(child_id);
        left.insert(cell.key, &ValueKind::Inline(&bytes))?;
    }
    Ok(())
}

fn update_separator(
    parent: &crate::v2::page::Page,
    left_pos: usize,
    new_separator: &[u8],
) -> Result<()> {
    if left_pos == 0 {
        return Err(Error::Corruption(format!(
            "update_separator called with left_pos=0 on page {}",
            parent.id
        )));
    }
    // The separator for the child at position left_pos and its right sibling is
    // stored in cell index left_pos - 1.
    let idx = left_pos - 1;
    let old_cell = parent.get_by_slot(idx)?;
    let child_id = decode_page_id(&old_cell.value)?;
    parent.write_slot(idx, Slot::deleted());
    parent.compact()?;
    let bytes = encode_page_id(child_id);
    parent.insert(new_separator, &ValueKind::Inline(&bytes))?;
    Ok(())
}

fn remove_child_at(parent: &crate::v2::page::Page, left_pos: usize) -> Result<()> {
    // Remove the separator whose right child is the child at position
    // left_pos + 1 (i.e., cell index left_pos).
    parent.write_slot(left_pos, Slot::deleted());
    parent.compact()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::buffer::BufferPool;
    use crate::v2::disk::PagedFile;
    use crate::v2::space::PageAllocator;
    use std::sync::Mutex;

    fn make_tree(page_size: usize, min_cells: usize) -> (BPlusTree, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let disk = Arc::new(PagedFile::open(dir.path().join("pages.dat"), page_size).unwrap());
        let alloc = Arc::new(Mutex::new(PageAllocator::new(1)));
        let pool = Arc::new(BufferPool::new(64, page_size, disk, alloc).unwrap());
        let tree = BPlusTree::with_min_cells(pool, page_size / 4, min_cells).unwrap();
        (tree, dir)
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
        let alloc = Arc::new(Mutex::new(PageAllocator::new(1)));
        let pool2 = Arc::new(BufferPool::new(64, 512, disk, alloc).unwrap());
        let tree2 = BPlusTree {
            pool: pool2,
            root_page_id: AtomicU64::new(tree.root_page_id()),
            inline_threshold: 128,
            min_cells: 1,
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
    fn large_value_rejected() {
        let (tree, _dir) = make_tree(512, 1);
        let big = vec![b'x'; 256];
        assert!(tree.insert(b"k", &big).is_err());
    }

    #[test]
    fn empty_key_rejected() {
        let (tree, _dir) = make_tree(512, 1);
        assert!(tree.insert(b"", b"v").is_err());
        assert!(tree.get(b"").is_err());
        assert!(tree.delete(b"").is_err());
    }

    #[test]
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
}
