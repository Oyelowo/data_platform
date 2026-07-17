//! B+ tree cursor.

use std::sync::Arc;

use bytes::Bytes;

use crate::engine::BtreeEngineInner;
use crate::error::{Error, Result};
use crate::node::{Node, NodeKind, Value};
use crate::page::{NULL_PAGE_ID, PageId};

/// Cursor over a key range in the B+ tree.
///
/// The cursor captures the root page id at creation time and traverses immutable
/// pages, so it presents a stable snapshot even if the engine is modified
/// concurrently.
pub struct BtreeCursor {
    inner: Arc<BtreeEngineInner>,
    root: PageId,
    end: Option<Bytes>,
    current_leaf: PageId,
    current_entries: Vec<(Bytes, Value)>,
    pos: usize,
    exhausted: bool,
}

impl BtreeCursor {
    pub(crate) fn new(
        inner: Arc<BtreeEngineInner>,
        root: PageId,
        start: Option<Bytes>,
        end: Option<Bytes>,
    ) -> Result<Self> {
        let mut cursor = Self {
            inner,
            root,
            end,
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
        let mut current_id = root;
        loop {
            let page = self.inner.pager.read(current_id)?;
            let node = Node::from_page(&page)?;
            match node.kind {
                NodeKind::Leaf { entries, next_leaf } => {
                    let idx =
                        target.map_or(0, |t| entries.partition_point(|(k, _)| k.as_ref() < t));
                    self.current_leaf = current_id;
                    self.current_entries = entries;
                    self.pos = idx;
                    self.exhausted =
                        next_leaf == NULL_PAGE_ID && self.pos >= self.current_entries.len();
                    return Ok(());
                }
                NodeKind::Internal { entries } => {
                    current_id = self.child_for_key(&entries, target);
                }
            }
        }
    }

    fn child_for_key(&self, entries: &[(Bytes, PageId)], key: Option<&[u8]>) -> PageId {
        match key {
            None => entries[0].1,
            Some(key) => {
                let mut child = entries[0].1;
                for (sep, cid) in entries.iter().skip(1) {
                    if key < sep.as_ref() {
                        return child;
                    }
                    child = *cid;
                }
                child
            }
        }
    }

    fn advance_leaf(&mut self) -> Result<bool> {
        if self.current_leaf == NULL_PAGE_ID {
            return Ok(false);
        }
        let page = self.inner.pager.read(self.current_leaf)?;
        let node = Node::from_page(&page)?;
        if let NodeKind::Leaf { next_leaf, .. } = node.kind {
            if next_leaf == NULL_PAGE_ID {
                self.exhausted = true;
                return Ok(false);
            }
            let next_page = self.inner.pager.read(next_leaf)?;
            let next_node = Node::from_page(&next_page)?;
            if let NodeKind::Leaf {
                entries,
                next_leaf: nn,
            } = next_node.kind
            {
                self.current_leaf = next_leaf;
                self.current_entries = entries;
                self.pos = 0;
                self.exhausted = nn == NULL_PAGE_ID && self.current_entries.is_empty();
                return Ok(true);
            }
            return Err(Error::Corruption("next_leaf is not a leaf".into()));
        }
        Err(Error::Corruption("current node is not a leaf".into()))
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
