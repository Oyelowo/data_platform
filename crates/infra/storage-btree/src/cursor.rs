//! B+ tree cursor.

use std::sync::Arc;

use bytes::Bytes;

use crate::engine::{BtreeEngineInner, SnapshotGuard};
use crate::error::{Error, Result};
use crate::node::{Node, NodeKind, Value};
use crate::page::{NULL_PAGE_ID, PageId};

/// Cursor over a key range in the B+ tree.
///
/// The cursor captures the root page id at creation time and traverses immutable
/// pages, so it presents a stable snapshot even if the engine is modified
/// concurrently. Forward leaf advancement uses a parent stack rather than leaf
/// sibling pointers, avoiding stale-sibling hazards after splits or merges.
pub struct BtreeCursor {
    inner: Arc<BtreeEngineInner>,
    root: PageId,
    /// Keeps the snapshot root pinned for the lifetime of the cursor.
    _guard: SnapshotGuard,
    end: Option<Bytes>,
    /// Path of internal nodes from the root down to `current_leaf`. Each entry
    /// is `(node_page_id, child_index_in_that_node)`.
    stack: Vec<(PageId, usize)>,
    current_leaf: PageId,
    current_entries: Vec<(Bytes, Value)>,
    pos: usize,
    exhausted: bool,
}

impl BtreeCursor {
    pub(crate) fn new(
        inner: Arc<BtreeEngineInner>,
        root: PageId,
        guard: SnapshotGuard,
        start: Option<Bytes>,
        end: Option<Bytes>,
    ) -> Result<Self> {
        let mut cursor = Self {
            inner,
            root,
            _guard: guard,
            end,
            stack: Vec::new(),
            current_leaf: NULL_PAGE_ID,
            current_entries: Vec::new(),
            pos: 0,
            exhausted: root == NULL_PAGE_ID,
        };
        if root != NULL_PAGE_ID {
            cursor.seek_to(root, start.as_deref())?;
        }
        Ok(cursor)
    }

    fn seek_to(&mut self, root: PageId, target: Option<&[u8]>) -> Result<()> {
        self.stack.clear();
        let mut current_id = root;
        loop {
            let page = self.inner.pager.read(current_id)?;
            let node = Node::from_page(&page)?;
            match node.kind {
                NodeKind::Leaf { entries, .. } => {
                    let idx =
                        target.map_or(0, |t| entries.partition_point(|(k, _)| k.as_ref() < t));
                    self.current_leaf = current_id;
                    self.current_entries = entries;
                    self.pos = idx;
                    self.exhausted = self.pos >= self.current_entries.len();
                    return Ok(());
                }
                NodeKind::Internal { entries } => {
                    let (child_idx, child_id) = self.pick_child(&entries, target);
                    self.stack.push((current_id, child_idx));
                    current_id = child_id;
                }
            }
        }
    }

    fn pick_child(&self, entries: &[(Bytes, PageId)], key: Option<&[u8]>) -> (usize, PageId) {
        match key {
            None => (0, entries[0].1),
            Some(key) => {
                let mut idx = 0;
                let mut child = entries[0].1;
                for (i, (sep, cid)) in entries.iter().enumerate().skip(1) {
                    if key < sep.as_ref() {
                        return (idx, child);
                    }
                    idx = i;
                    child = *cid;
                }
                (idx, child)
            }
        }
    }

    fn advance_leaf(&mut self) -> Result<bool> {
        if self.current_leaf == NULL_PAGE_ID {
            self.exhausted = true;
            return Ok(false);
        }
        // Ascend the parent stack until we find an ancestor with a right sibling.
        while let Some((parent_id, child_idx)) = self.stack.pop() {
            let parent_page = self.inner.pager.read(parent_id)?;
            let parent_node = Node::from_page(&parent_page)?;
            if let NodeKind::Internal { entries } = &parent_node.kind {
                if child_idx + 1 < entries.len() {
                    let next_child = entries[child_idx + 1].1;
                    self.stack.push((parent_id, child_idx + 1));
                    self.descend_to_leftmost_leaf(next_child)?;
                    return Ok(true);
                }
            } else {
                return Err(Error::Corruption(
                    "cursor stack contains a non-internal node".into(),
                ));
            }
        }
        self.exhausted = true;
        Ok(false)
    }

    fn descend_to_leftmost_leaf(&mut self, mut current_id: PageId) -> Result<()> {
        loop {
            let page = self.inner.pager.read(current_id)?;
            let node = Node::from_page(&page)?;
            match node.kind {
                NodeKind::Leaf { entries, .. } => {
                    self.current_leaf = current_id;
                    self.current_entries = entries;
                    self.pos = 0;
                    self.exhausted = self.current_entries.is_empty();
                    return Ok(());
                }
                NodeKind::Internal { entries } => {
                    let child_id = entries[0].1;
                    self.stack.push((current_id, 0));
                    current_id = child_id;
                }
            }
        }
    }
}

impl Iterator for BtreeCursor {
    type Item = Result<(Bytes, Bytes)>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.exhausted {
            return None;
        }
        loop {
            if self.pos >= self.current_entries.len() {
                match self.advance_leaf() {
                    Ok(true) => continue,
                    Ok(false) => return None,
                    Err(e) => return Some(Err(e)),
                }
            }
            let (key, value) = &self.current_entries[self.pos];
            if let Some(ref end) = self.end
                && key.as_ref() >= end.as_ref()
            {
                self.exhausted = true;
                return None;
            }
            let key = key.clone();
            let value = match self.inner.pager.resolve_value(value) {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            self.pos += 1;
            return Some(Ok((key, value)));
        }
    }
}

impl storage_traits::Cursor for BtreeCursor {
    type Error = Error;

    fn seek(&mut self, target: &[u8]) -> Result<()> {
        self.exhausted = false;
        if self.root == NULL_PAGE_ID {
            self.exhausted = true;
            return Ok(());
        }
        self.seek_to(self.root, Some(target))
    }
}
