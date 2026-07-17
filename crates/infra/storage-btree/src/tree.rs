//! Copy-on-write B+ tree operations.

use std::collections::HashSet;
use std::sync::Arc;

use bytes::Bytes;

use crate::error::{Error, Result};
use crate::node::{Node, NodeKind, Value};
use crate::options::BtreeOptions;
use crate::page::{NULL_PAGE_ID, PageId};
use crate::pager::Pager;

/// A copy-on-write B+ tree.
pub(crate) struct Tree {
    pager: Arc<Pager>,
    page_size: usize,
    min_fill_percent: usize,
    max_inline_value_size: usize,
    max_value_size: usize,
}

/// Result of a COW deletion that may propagate a merge/redistribution upward.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DeleteResult {
    /// Node size is healthy.
    Balanced,
    /// Node became empty and should be removed by the parent.
    Empty,
    /// Node underflowed and needs to borrow/merge with a sibling.
    Underflow,
}

impl Tree {
    /// Create a new tree backed by `pager` and configured by `options`.
    pub fn new(pager: Arc<Pager>, options: &BtreeOptions) -> Self {
        Self {
            pager,
            page_size: options.page_size,
            min_fill_percent: options.min_fill_percent,
            max_inline_value_size: options.max_inline_value_size,
            max_value_size: options.max_value_size,
        }
    }

    /// Search for a key in the tree rooted at `root_id`.
    pub fn get(&self, root_id: PageId, key: &[u8]) -> Result<Option<Bytes>> {
        if root_id == NULL_PAGE_ID {
            return Ok(None);
        }
        let mut current_id = root_id;
        loop {
            let page = self.pager.read(current_id)?;
            let node = Node::from_page(&page)?;
            match node.kind {
                NodeKind::Leaf { entries, .. } => {
                    return entries
                        .binary_search_by(|(k, _)| k.as_ref().cmp(key))
                        .ok()
                        .map(|idx| self.pager.resolve_value(&entries[idx].1))
                        .transpose();
                }
                NodeKind::Internal { entries } => {
                    current_id = self.child_for_key(&entries, key);
                }
            }
        }
    }

    /// Insert or replace a key/value pair, returning the new root page id.
    pub fn insert(&self, root_id: PageId, key: &[u8], value: &[u8]) -> Result<PageId> {
        self.check_key_size(key)?;
        let value = self.encode_value(value)?;
        let entry_size = Node::leaf_entry_size(key.len(), &value) + 8;
        if entry_size > self.usable_page_size() {
            return Err(Error::OutOfBounds {
                kind: crate::error::BoundKind::Value,
                limit: self.usable_page_size().saturating_sub(8),
                got: entry_size.saturating_sub(8),
            });
        }
        if root_id == NULL_PAGE_ID {
            let mut leaf = Node::empty_leaf(self.pager.allocate());
            if let NodeKind::Leaf {
                ref mut entries, ..
            } = leaf.kind
            {
                entries.push((Bytes::copy_from_slice(key), value));
            }
            self.write_node(&leaf)?;
            return Ok(leaf.id);
        }

        let (new_root, split) = self.insert_recursive(root_id, key, value)?;
        match split {
            None => Ok(new_root),
            Some((separator, right_id)) => {
                let mut new_root = Node::empty_internal(self.pager.allocate(), new_root);
                if let NodeKind::Internal { entries: ref mut e } = new_root.kind {
                    e.push((separator, right_id));
                }
                self.write_node(&new_root)?;
                self.pager.retire(root_id);
                Ok(new_root.id)
            }
        }
    }

    /// Delete a key, returning the new root page id.
    pub fn delete(&self, root_id: PageId, key: &[u8]) -> Result<PageId> {
        if root_id == NULL_PAGE_ID {
            return Ok(NULL_PAGE_ID);
        }
        let (new_root, result) = self.delete_recursive(root_id, key)?;
        if new_root == NULL_PAGE_ID || result == DeleteResult::Empty {
            return Ok(NULL_PAGE_ID);
        }

        // If the root underflowed and is an internal node with a single child,
        // shrink the tree by promoting that child.
        if result == DeleteResult::Underflow {
            let page = self.pager.read(new_root)?;
            let node = Node::from_page(&page)?;
            if let NodeKind::Internal { entries } = &node.kind
                && entries.len() == 1
            {
                let child_id = entries[0].1;
                self.pager.retire(new_root);
                return Ok(child_id);
            }
        }
        Ok(new_root)
    }

    /// Return an iterator over the entries in `[start, end)`.
    #[cfg(test)]
    pub fn scan(
        &self,
        root_id: PageId,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
    ) -> Result<TreeIter> {
        TreeIter::new(Arc::clone(&self.pager), root_id, start, end)
    }

    /// Validate the structural integrity of the tree reachable from `root_id`.
    ///
    /// Checks page decoding, key ordering, separator correctness, bounds against
    /// parent separators, overflow chain acyclicity, and the absence of shared
    /// or cycled pages in the reachable tree. Leaf sibling pointers are not
    /// relied on for correctness.
    pub fn check_integrity(&self, root_id: PageId) -> Result<()> {
        if root_id == NULL_PAGE_ID {
            return Ok(());
        }

        let mut visited = HashSet::new();
        let mut stack = vec![(root_id, None::<Bytes>, None::<Bytes>)];

        while let Some((page_id, low, high)) = stack.pop() {
            if !visited.insert(page_id) {
                return Err(Error::Corruption(format!(
                    "page {page_id} reachable via multiple paths or cycle"
                )));
            }

            let page = self.pager.read(page_id)?;
            let node = Node::from_page(&page)?;

            match &node.kind {
                NodeKind::Leaf { entries, .. } => {
                    for (i, (key, value)) in entries.iter().enumerate() {
                        if i > 0 && key.as_ref() <= entries[i - 1].0.as_ref() {
                            return Err(Error::Corruption(format!(
                                "leaf {page_id} keys out of order at index {i}"
                            )));
                        }
                        if let Some(ref low_key) = low
                            && key.as_ref() < low_key.as_ref()
                        {
                            return Err(Error::Corruption(format!(
                                "leaf {page_id} key below lower bound at index {i}"
                            )));
                        }
                        if let Some(ref high_key) = high
                            && key.as_ref() >= high_key.as_ref()
                        {
                            return Err(Error::Corruption(format!(
                                "leaf {page_id} key at/above upper bound at index {i}"
                            )));
                        }
                        if let Value::Overflow(head) = value {
                            self.pager.validate_overflow(*head)?;
                        }
                    }
                }
                NodeKind::Internal { entries } => {
                    if entries.is_empty() {
                        return Err(Error::Corruption(format!(
                            "internal {page_id} has no entries"
                        )));
                    }
                    if !entries[0].0.is_empty() {
                        return Err(Error::Corruption(format!(
                            "internal {page_id} leftmost separator is not empty"
                        )));
                    }
                    for i in 1..entries.len() {
                        if entries[i].0.as_ref() <= entries[i - 1].0.as_ref() {
                            return Err(Error::Corruption(format!(
                                "internal {page_id} separators out of order at index {i}"
                            )));
                        }
                    }
                    for i in 0..entries.len() {
                        let child_low = if i == 0 {
                            low.clone()
                        } else {
                            Some(entries[i].0.clone())
                        };
                        let child_high = if i + 1 < entries.len() {
                            Some(entries[i + 1].0.clone())
                        } else {
                            high.clone()
                        };
                        stack.push((entries[i].1, child_low, child_high));
                    }
                }
            }
        }

        Ok(())
    }

    fn encode_value(&self, value: &[u8]) -> Result<Value> {
        if value.len() > self.max_value_size {
            return Err(Error::OutOfBounds {
                kind: crate::error::BoundKind::Value,
                limit: self.max_value_size,
                got: value.len(),
            });
        }
        if value.len() > self.max_inline_value_size {
            let head = self.pager.write_overflow(value)?;
            Ok(Value::Overflow(head))
        } else {
            Ok(Value::Inline(Bytes::copy_from_slice(value)))
        }
    }

    fn write_node(&self, node: &Node) -> Result<()> {
        let mut body = bytes::BytesMut::with_capacity(self.page_size - 4);
        node.serialize(&mut body, self.page_size)?;
        let page = crate::page::Page::build(node.id, body, self.page_size)?;
        self.pager.write(&page)
    }

    fn child_for_key(&self, entries: &[(Bytes, PageId)], key: &[u8]) -> PageId {
        let mut child = entries[0].1;
        for (sep, cid) in entries.iter().skip(1) {
            if key < sep.as_ref() {
                return child;
            }
            child = *cid;
        }
        child
    }

    fn pick_child(&self, entries: &[(Bytes, PageId)], key: &[u8]) -> (usize, PageId) {
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

    fn insert_recursive(
        &self,
        node_id: PageId,
        key: &[u8],
        value: Value,
    ) -> Result<(PageId, Option<(Bytes, PageId)>)> {
        let page = self.pager.read(node_id)?;
        let node = Node::from_page(&page)?;

        match &node.kind {
            NodeKind::Leaf { next_leaf, entries } => {
                let mut new_entries = entries.clone();
                match new_entries.binary_search_by(|(k, _)| k.as_ref().cmp(key)) {
                    Ok(idx) => {
                        let old_value = std::mem::replace(&mut new_entries[idx].1, value);
                        self.free_value(&old_value);
                    }
                    Err(idx) => {
                        new_entries.insert(idx, (Bytes::copy_from_slice(key), value));
                    }
                }

                let mut new_leaf = Node::empty_leaf(self.pager.allocate());
                if let NodeKind::Leaf {
                    entries: ref mut e,
                    next_leaf: ref mut nl,
                } = new_leaf.kind
                {
                    *e = new_entries;
                    *nl = *next_leaf;
                }
                let split = self.split_leaf_if_needed(&mut new_leaf)?;
                self.write_node(&new_leaf)?;
                self.pager.retire(node_id);

                match split {
                    None => Ok((new_leaf.id, None)),
                    Some((separator, right_id)) => Ok((new_leaf.id, Some((separator, right_id)))),
                }
            }
            NodeKind::Internal { entries } => {
                let (child_idx, child_id) = self.pick_child(entries, key);
                let (new_child_id, child_split) = self.insert_recursive(child_id, key, value)?;
                if new_child_id == child_id && child_split.is_none() {
                    return Ok((node_id, None));
                }

                let mut new_entries = entries.clone();
                new_entries[child_idx].1 = new_child_id;
                if let Some((separator, right_id)) = child_split {
                    new_entries.insert(child_idx + 1, (separator, right_id));
                }
                let mut new_internal =
                    Node::empty_internal(self.pager.allocate(), new_entries[0].1);
                if let NodeKind::Internal { entries: ref mut e } = new_internal.kind {
                    *e = new_entries;
                }
                let split = self.split_internal_if_needed(&mut new_internal)?;
                self.write_node(&new_internal)?;
                self.pager.retire(node_id);

                match split {
                    None => Ok((new_internal.id, None)),
                    Some((separator, right_id)) => {
                        Ok((new_internal.id, Some((separator, right_id))))
                    }
                }
            }
        }
    }

    fn split_leaf_if_needed(&self, node: &mut Node) -> Result<Option<(Bytes, PageId)>> {
        let entries = match &mut node.kind {
            NodeKind::Leaf { entries, .. } => entries,
            _ => return Ok(None),
        };
        if self.leaf_entries_fit(entries) {
            return Ok(None);
        }

        let mut split_point = entries.len() / 2;
        split_point = split_point.max(1).min(entries.len().saturating_sub(1));

        // Find a split point where both halves fit. This loop is bounded because
        // a single entry is guaranteed to fit after `check_key_size` and the
        // overflow path for large values.
        loop {
            if self.leaf_entries_fit(&entries[..split_point])
                && self.leaf_entries_fit(&entries[split_point..])
            {
                break;
            }
            if split_point + 1 < entries.len() {
                split_point += 1;
            } else if split_point > 1 {
                split_point -= 1;
            } else {
                return Err(Error::Corruption(
                    "cannot split leaf: single entry does not fit in page".into(),
                ));
            }
        }

        let old_entries = std::mem::take(entries);
        let mut left_entries = old_entries;
        let right_entries = left_entries.split_off(split_point);
        let separator = right_entries[0].0.clone();
        let right_id = self.pager.allocate();
        let old_next_leaf = match &node.kind {
            NodeKind::Leaf { next_leaf, .. } => *next_leaf,
            _ => NULL_PAGE_ID,
        };

        let mut right = Node::empty_leaf(right_id);
        if let NodeKind::Leaf {
            ref mut entries,
            ref mut next_leaf,
        } = right.kind
        {
            *entries = right_entries;
            *next_leaf = old_next_leaf;
        }
        self.write_node(&right)?;

        if let NodeKind::Leaf {
            ref mut entries,
            ref mut next_leaf,
        } = node.kind
        {
            *entries = left_entries;
            *next_leaf = right_id;
        }
        Ok(Some((separator, right_id)))
    }

    fn split_internal_if_needed(&self, node: &mut Node) -> Result<Option<(Bytes, PageId)>> {
        let entries = match &mut node.kind {
            NodeKind::Internal { entries } => entries,
            _ => return Ok(None),
        };
        if self.internal_entries_fit(entries) {
            return Ok(None);
        }

        let mut split_point = entries.len() / 2;
        split_point = split_point.max(1).min(entries.len().saturating_sub(1));

        loop {
            if self.internal_entries_fit(&entries[..split_point])
                && self.internal_entries_fit(&entries[split_point..])
            {
                break;
            }
            if split_point + 1 < entries.len() {
                split_point += 1;
            } else if split_point > 1 {
                split_point -= 1;
            } else {
                return Err(Error::Corruption(
                    "cannot split internal: single entry does not fit in page".into(),
                ));
            }
        }

        let old_entries = std::mem::take(entries);
        let mut left_entries = old_entries;
        let mut right_entries = left_entries.split_off(split_point);
        // The separator promoted to the parent is the key that used to separate
        // the left and right halves. In the right node it becomes the empty
        // leftmost separator.
        let separator = right_entries[0].0.clone();
        right_entries[0].0 = Bytes::new();
        let right_id = self.pager.allocate();

        let mut right = Node::empty_internal(right_id, right_entries[0].1);
        if let NodeKind::Internal { entries: ref mut e } = right.kind {
            *e = right_entries;
        }
        self.write_node(&right)?;

        if let NodeKind::Internal { entries: ref mut e } = node.kind {
            *e = left_entries;
        }
        Ok(Some((separator, right_id)))
    }

    fn delete_recursive(&self, node_id: PageId, key: &[u8]) -> Result<(PageId, DeleteResult)> {
        let page = self.pager.read(node_id)?;
        let node = Node::from_page(&page)?;

        match &node.kind {
            NodeKind::Leaf { next_leaf, entries } => {
                let mut new_entries = entries.clone();
                let found = match new_entries.binary_search_by(|(k, _)| k.as_ref().cmp(key)) {
                    Ok(idx) => {
                        let (_, old_value) = new_entries.remove(idx);
                        self.free_value(&old_value);
                        true
                    }
                    Err(_) => false,
                };
                if !found {
                    return Ok((node_id, DeleteResult::Balanced));
                }
                if new_entries.is_empty() {
                    self.free_node_values(&node);
                    self.pager.retire(node_id);
                    return Ok((NULL_PAGE_ID, DeleteResult::Empty));
                }

                let mut new_leaf = Node::empty_leaf(self.pager.allocate());
                if let NodeKind::Leaf {
                    entries: ref mut e,
                    next_leaf: ref mut nl,
                } = new_leaf.kind
                {
                    *e = new_entries;
                    *nl = *next_leaf;
                }
                self.write_node(&new_leaf)?;
                self.pager.retire(node_id);

                let result = if self.below_min_leaf(&new_leaf) {
                    DeleteResult::Underflow
                } else {
                    DeleteResult::Balanced
                };
                Ok((new_leaf.id, result))
            }
            NodeKind::Internal { entries } => {
                let (child_idx, child_id) = self.pick_child(entries, key);
                let (new_child_id, child_result) = self.delete_recursive(child_id, key)?;
                if new_child_id == child_id && child_result == DeleteResult::Balanced {
                    return Ok((node_id, DeleteResult::Balanced));
                }

                match child_result {
                    DeleteResult::Balanced => {
                        let mut new_entries = entries.clone();
                        new_entries[child_idx].1 = new_child_id;
                        self.finish_internal(node_id, new_entries)
                    }
                    DeleteResult::Empty => {
                        let mut new_entries = entries.clone();
                        new_entries.remove(child_idx);
                        if new_entries.is_empty() {
                            self.pager.retire(node_id);
                            return Ok((NULL_PAGE_ID, DeleteResult::Empty));
                        }
                        // The new leftmost child must have an empty separator.
                        new_entries[0].0 = Bytes::new();
                        self.finish_internal(node_id, new_entries)
                    }
                    DeleteResult::Underflow => {
                        let child_page = self.pager.read(new_child_id)?;
                        let child_node = Node::from_page(&child_page)?;
                        if child_node.is_leaf() {
                            self.rebalance_leaf(node_id, entries, child_idx, new_child_id)
                        } else {
                            self.rebalance_internal(node_id, entries, child_idx, new_child_id)
                        }
                    }
                }
            }
        }
    }

    fn finish_internal(
        &self,
        node_id: PageId,
        new_entries: Vec<(Bytes, PageId)>,
    ) -> Result<(PageId, DeleteResult)> {
        let mut new_internal = Node::empty_internal(self.pager.allocate(), new_entries[0].1);
        if let NodeKind::Internal { entries: ref mut e } = new_internal.kind {
            *e = new_entries;
        }
        self.write_node(&new_internal)?;
        self.pager.retire(node_id);
        let result = if self.below_min_internal(&new_internal) {
            DeleteResult::Underflow
        } else {
            DeleteResult::Balanced
        };
        Ok((new_internal.id, result))
    }

    fn rebalance_leaf(
        &self,
        old_node_id: PageId,
        parent_entries: &[(Bytes, PageId)],
        child_idx: usize,
        underflow_child_id: PageId,
    ) -> Result<(PageId, DeleteResult)> {
        if parent_entries.len() == 1 {
            // The parent has only this child; propagate the underflow upward.
            let mut new_parent_entries = parent_entries.to_vec();
            new_parent_entries[child_idx].1 = underflow_child_id;
            return self.finish_internal(old_node_id, new_parent_entries);
        }

        let (sibling_idx, sibling_id) = self.pick_sibling(parent_entries, child_idx);

        let underflow_page = self.pager.read(underflow_child_id)?;
        let underflow_node = Node::from_page(&underflow_page)?;
        let sibling_page = self.pager.read(sibling_id)?;
        let sibling_node = Node::from_page(&sibling_page)?;

        let (underflow_entries, underflow_next) = match underflow_node.kind {
            NodeKind::Leaf { entries, next_leaf } => (entries, next_leaf),
            _ => return Err(Error::Corruption("underflow child is not a leaf".into())),
        };
        let (sibling_entries, sibling_next) = match sibling_node.kind {
            NodeKind::Leaf { entries, next_leaf } => (entries, next_leaf),
            _ => return Err(Error::Corruption("sibling is not a leaf".into())),
        };

        let mut all_entries = Vec::with_capacity(underflow_entries.len() + sibling_entries.len());
        let (left_old_id, right_old_id, right_next) = if sibling_idx < child_idx {
            // Sibling is on the left, underflow is on the right.
            all_entries.extend_from_slice(&sibling_entries);
            all_entries.extend_from_slice(&underflow_entries);
            (sibling_id, underflow_child_id, underflow_next)
        } else {
            // Underflow is on the left, sibling is on the right.
            all_entries.extend_from_slice(&underflow_entries);
            all_entries.extend_from_slice(&sibling_entries);
            (underflow_child_id, sibling_id, sibling_next)
        };

        let (left_idx, right_idx) = if sibling_idx < child_idx {
            (sibling_idx, child_idx)
        } else {
            (child_idx, sibling_idx)
        };

        if self.leaf_entries_fit(&all_entries) {
            // Merge the two leaves into one.
            let merged_id = self.pager.allocate();
            let mut merged = Node::empty_leaf(merged_id);
            if let NodeKind::Leaf {
                ref mut entries,
                ref mut next_leaf,
            } = merged.kind
            {
                *entries = all_entries;
                *next_leaf = right_next;
            }
            self.write_node(&merged)?;

            let mut new_parent_entries = parent_entries.to_vec();
            new_parent_entries[left_idx].1 = merged_id;
            new_parent_entries.remove(right_idx);
            return self.finish_parent(old_node_id, &new_parent_entries, left_old_id, right_old_id);
        }

        // Redistribute entries evenly between two new leaves.
        let split_point = all_entries.len() / 2;
        let left_entries = all_entries[..split_point].to_vec();
        let right_entries = all_entries[split_point..].to_vec();
        let separator = right_entries[0].0.clone();

        let right_id = self.pager.allocate();
        let mut right = Node::empty_leaf(right_id);
        if let NodeKind::Leaf {
            ref mut entries,
            ref mut next_leaf,
        } = right.kind
        {
            *entries = right_entries;
            *next_leaf = right_next;
        }

        let left_id = self.pager.allocate();
        let mut left = Node::empty_leaf(left_id);
        if let NodeKind::Leaf {
            ref mut entries,
            ref mut next_leaf,
        } = left.kind
        {
            *entries = left_entries;
            *next_leaf = right_id;
        }
        self.write_node(&left)?;
        self.write_node(&right)?;

        let mut new_parent_entries = parent_entries.to_vec();
        new_parent_entries[left_idx].1 = left_id;
        new_parent_entries[right_idx].1 = right_id;
        new_parent_entries[right_idx].0 = separator;
        self.finish_parent(old_node_id, &new_parent_entries, left_old_id, right_old_id)
    }

    fn rebalance_internal(
        &self,
        old_node_id: PageId,
        parent_entries: &[(Bytes, PageId)],
        child_idx: usize,
        underflow_child_id: PageId,
    ) -> Result<(PageId, DeleteResult)> {
        if parent_entries.len() == 1 {
            // The parent has only this child; propagate the underflow upward.
            let mut new_parent_entries = parent_entries.to_vec();
            new_parent_entries[child_idx].1 = underflow_child_id;
            return self.finish_internal(old_node_id, new_parent_entries);
        }

        let (sibling_idx, sibling_id) = self.pick_sibling(parent_entries, child_idx);

        let underflow_page = self.pager.read(underflow_child_id)?;
        let underflow_node = Node::from_page(&underflow_page)?;
        let sibling_page = self.pager.read(sibling_id)?;
        let sibling_node = Node::from_page(&sibling_page)?;

        let underflow_entries = match underflow_node.kind {
            NodeKind::Internal { entries } => entries,
            _ => {
                return Err(Error::Corruption(
                    "underflow child is not an internal node".into(),
                ));
            }
        };
        let sibling_entries = match sibling_node.kind {
            NodeKind::Internal { entries } => entries,
            _ => return Err(Error::Corruption("sibling is not an internal node".into())),
        };

        // The parent separator that currently sits between the two children.
        let separator_idx = child_idx.max(sibling_idx);
        let parent_separator = parent_entries[separator_idx].0.clone();

        let mut all_entries = Vec::with_capacity(
            underflow_entries
                .len()
                .saturating_add(sibling_entries.len())
                .saturating_add(1),
        );
        let (left_old_id, right_old_id) = if sibling_idx < child_idx {
            // Sibling is on the left.
            all_entries.extend_from_slice(&sibling_entries);
            all_entries.push((parent_separator, underflow_entries[0].1));
            all_entries.extend_from_slice(&underflow_entries[1..]);
            (sibling_id, underflow_child_id)
        } else {
            // Sibling is on the right.
            all_entries.extend_from_slice(&underflow_entries);
            all_entries.push((parent_separator, sibling_entries[0].1));
            all_entries.extend_from_slice(&sibling_entries[1..]);
            (underflow_child_id, sibling_id)
        };

        let (left_idx, right_idx) = if sibling_idx < child_idx {
            (sibling_idx, child_idx)
        } else {
            (child_idx, sibling_idx)
        };

        if self.internal_entries_fit(&all_entries) {
            // Merge the two internal nodes into one.
            let merged_id = self.pager.allocate();
            let mut merged = Node::empty_internal(merged_id, all_entries[0].1);
            if let NodeKind::Internal { entries: ref mut e } = merged.kind {
                *e = all_entries;
            }
            self.write_node(&merged)?;

            let mut new_parent_entries = parent_entries.to_vec();
            new_parent_entries[left_idx].1 = merged_id;
            new_parent_entries.remove(right_idx);
            return self.finish_parent(old_node_id, &new_parent_entries, left_old_id, right_old_id);
        }

        // Redistribute entries evenly between two new internal nodes.
        let split_point = all_entries.len() / 2;
        let left_entries = all_entries[..split_point].to_vec();
        let mut right_entries = all_entries[split_point..].to_vec();
        let new_separator = right_entries[0].0.clone();
        right_entries[0].0 = Bytes::new();

        let left_id = self.pager.allocate();
        let mut left = Node::empty_internal(left_id, left_entries[0].1);
        if let NodeKind::Internal { entries: ref mut e } = left.kind {
            *e = left_entries;
        }
        let right_id = self.pager.allocate();
        let mut right = Node::empty_internal(right_id, right_entries[0].1);
        if let NodeKind::Internal { entries: ref mut e } = right.kind {
            *e = right_entries;
        }
        self.write_node(&left)?;
        self.write_node(&right)?;

        let mut new_parent_entries = parent_entries.to_vec();
        new_parent_entries[left_idx].1 = left_id;
        new_parent_entries[right_idx].1 = right_id;
        new_parent_entries[right_idx].0 = new_separator;
        self.finish_parent(old_node_id, &new_parent_entries, left_old_id, right_old_id)
    }

    fn finish_parent(
        &self,
        old_node_id: PageId,
        new_parent_entries: &[(Bytes, PageId)],
        left_old_id: PageId,
        right_old_id: PageId,
    ) -> Result<(PageId, DeleteResult)> {
        if new_parent_entries.is_empty() {
            self.pager.retire(old_node_id);
            self.pager.retire(left_old_id);
            self.pager.retire(right_old_id);
            return Ok((NULL_PAGE_ID, DeleteResult::Empty));
        }

        let mut new_parent = Node::empty_internal(self.pager.allocate(), new_parent_entries[0].1);
        if let NodeKind::Internal { entries: ref mut e } = new_parent.kind {
            *e = new_parent_entries.to_vec();
        }
        self.write_node(&new_parent)?;
        self.pager.retire(old_node_id);
        self.pager.retire(left_old_id);
        self.pager.retire(right_old_id);

        let result = if self.below_min_internal(&new_parent) {
            DeleteResult::Underflow
        } else {
            DeleteResult::Balanced
        };
        Ok((new_parent.id, result))
    }

    fn pick_sibling(
        &self,
        parent_entries: &[(Bytes, PageId)],
        child_idx: usize,
    ) -> (usize, PageId) {
        // Prefer the left sibling because it keeps the separator update simple.
        if child_idx > 0 {
            (child_idx - 1, parent_entries[child_idx - 1].1)
        } else {
            (child_idx + 1, parent_entries[child_idx + 1].1)
        }
    }

    fn leaf_entries_fit(&self, entries: &[(Bytes, Value)]) -> bool {
        let used = 8usize
            + entries
                .iter()
                .map(|(k, v)| Node::leaf_entry_size(k.len(), v))
                .sum::<usize>();
        used <= self.usable_page_size()
    }

    fn internal_entries_fit(&self, entries: &[(Bytes, PageId)]) -> bool {
        let used = entries
            .iter()
            .map(|(k, _)| Node::internal_entry_size(k.len()))
            .sum::<usize>();
        used <= self.usable_page_size()
    }

    fn usable_page_size(&self) -> usize {
        // The serialized body is `page_size - 4` bytes; the fixed header fields
        // occupy `PageHeader::SIZE` bytes; the remaining space is for payload.
        self.page_size - crate::page::PageHeader::SIZE - 4
    }

    fn below_min_leaf(&self, node: &Node) -> bool {
        let entries = match &node.kind {
            NodeKind::Leaf { entries, .. } => entries,
            _ => return false,
        };
        let used = 8usize
            + entries
                .iter()
                .map(|(k, v)| Node::leaf_entry_size(k.len(), v))
                .sum::<usize>();
        used < self.min_node_size()
    }

    fn below_min_internal(&self, node: &Node) -> bool {
        let entries = match &node.kind {
            NodeKind::Internal { entries } => entries,
            _ => return false,
        };
        let used = entries
            .iter()
            .map(|(k, _)| Node::internal_entry_size(k.len()))
            .sum::<usize>();
        used < self.min_node_size()
    }

    fn min_node_size(&self) -> usize {
        self.usable_page_size() * self.min_fill_percent / 100
    }

    fn check_key_size(&self, key: &[u8]) -> Result<()> {
        if key.len() > u16::MAX as usize {
            return Err(Error::OutOfBounds {
                kind: crate::error::BoundKind::Key,
                limit: u16::MAX as usize,
                got: key.len(),
            });
        }
        Ok(())
    }

    fn free_value(&self, value: &Value) {
        if let Value::Overflow(head) = value {
            self.pager.retire_overflow(*head);
        }
    }

    fn free_node_values(&self, node: &Node) {
        if let NodeKind::Leaf { entries, .. } = &node.kind {
            for (_, value) in entries {
                self.free_value(value);
            }
        }
    }
}

/// Iterator over a range of entries in the B+ tree.
///
/// Forward advancement uses a parent stack rather than leaf sibling pointers,
/// avoiding stale-sibling hazards after splits or merges.
#[cfg(test)]
pub(crate) struct TreeIter {
    pager: Arc<Pager>,
    end: Option<Bytes>,
    /// Path of internal nodes from the root down to `current_leaf`. Each entry
    /// is `(node_page_id, child_index_in_that_node)`.
    stack: Vec<(PageId, usize)>,
    current_leaf: PageId,
    current_entries: Vec<(Bytes, Value)>,
    pos: usize,
    exhausted: bool,
}

#[cfg(test)]
impl TreeIter {
    fn new(
        pager: Arc<Pager>,
        root_id: PageId,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
    ) -> Result<Self> {
        let mut iter = Self {
            pager,
            end: end.map(Bytes::copy_from_slice),
            stack: Vec::new(),
            current_leaf: NULL_PAGE_ID,
            current_entries: Vec::new(),
            pos: 0,
            exhausted: root_id == NULL_PAGE_ID,
        };
        if root_id != NULL_PAGE_ID {
            iter.seek(root_id, start)?;
        }
        Ok(iter)
    }

    fn seek(&mut self, root_id: PageId, target: Option<&[u8]>) -> Result<()> {
        self.stack.clear();
        let mut current_id = root_id;
        loop {
            let page = self.pager.read(current_id)?;
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
            let parent_page = self.pager.read(parent_id)?;
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
                    "iterator stack contains a non-internal node".into(),
                ));
            }
        }
        self.exhausted = true;
        Ok(false)
    }

    fn descend_to_leftmost_leaf(&mut self, mut current_id: PageId) -> Result<()> {
        loop {
            let page = self.pager.read(current_id)?;
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

#[cfg(test)]
impl Iterator for TreeIter {
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
            let value = match self.pager.resolve_value(value) {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            self.pos += 1;
            return Some(Ok((key, value)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tree(page_size: usize) -> (Tree, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let pager = Arc::new(Pager::open(dir.path(), page_size, 0).unwrap());
        let options = BtreeOptions {
            page_size,
            max_inline_value_size: page_size / 4,
            min_fill_percent: 50,
            cache_size: 0,
            max_value_size: 16 * 1024 * 1024,
            max_batch_ops: 10_000,
        };
        let tree = Tree::new(pager, &options);
        (tree, dir)
    }

    #[test]
    fn insert_and_get() {
        let (tree, _dir) = make_tree(512);
        let mut root = NULL_PAGE_ID;
        root = tree.insert(root, b"a", b"1").unwrap();
        root = tree.insert(root, b"b", b"2").unwrap();
        assert_eq!(
            tree.get(root, b"a").unwrap(),
            Some(Bytes::from_static(b"1"))
        );
        assert_eq!(
            tree.get(root, b"b").unwrap(),
            Some(Bytes::from_static(b"2"))
        );
        assert_eq!(tree.get(root, b"c").unwrap(), None);
    }

    #[test]
    fn delete_key() {
        let (tree, _dir) = make_tree(512);
        let mut root = NULL_PAGE_ID;
        root = tree.insert(root, b"a", b"1").unwrap();
        root = tree.insert(root, b"b", b"2").unwrap();
        root = tree.delete(root, b"a").unwrap();
        assert_eq!(tree.get(root, b"a").unwrap(), None);
        assert_eq!(
            tree.get(root, b"b").unwrap(),
            Some(Bytes::from_static(b"2"))
        );
    }

    #[test]
    fn scan_range() {
        let (tree, _dir) = make_tree(512);
        let mut root = NULL_PAGE_ID;
        for i in 0..10u8 {
            root = tree.insert(root, &[i], &[i + 100]).unwrap();
        }
        let cursor = tree.scan(root, Some(&[3u8]), Some(&[7u8])).unwrap();
        let items: Vec<_> = cursor.map(|r| r.unwrap()).collect();
        assert_eq!(items.len(), 4);
        assert_eq!(items[0].0.as_ref(), &[3]);
        assert_eq!(items[3].0.as_ref(), &[6]);
    }

    #[test]
    fn many_inserts_and_deletes() {
        let (tree, _dir) = make_tree(512);
        let mut root = NULL_PAGE_ID;
        for i in 0..100u8 {
            root = tree.insert(root, &[i], &[i]).unwrap();
        }
        for i in 0..100u8 {
            assert_eq!(tree.get(root, &[i]).unwrap(), Some(Bytes::from(vec![i])));
        }
        for i in 0..100u8 {
            root = tree.delete(root, &[i]).unwrap();
        }
        for i in 0..100u8 {
            assert_eq!(tree.get(root, &[i]).unwrap(), None);
        }
        assert_eq!(root, NULL_PAGE_ID);
    }

    #[test]
    fn overwrite_frees_overflow() {
        let (tree, _dir) = make_tree(512);
        let mut root = NULL_PAGE_ID;
        let large = vec![0xABu8; 4096];
        root = tree.insert(root, b"k", &large).unwrap();
        root = tree.insert(root, b"k", b"small").unwrap();
        assert_eq!(
            tree.get(root, b"k").unwrap(),
            Some(Bytes::from_static(b"small"))
        );
    }
}
