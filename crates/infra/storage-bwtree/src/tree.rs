//! Bw-Tree operations: lock-free point reads/writes and structural
//! modifications.
//!
//! Record updates are installed with CAS loops on mapping-table entries.
//! Structural modifications (splits and merges) are serialized by a global SMO
//! lock to avoid the full ∆abort protocol in the first version.

use std::sync::Arc;

use bytes::Bytes;
use crossbeam_epoch::{self as epoch};
use parking_lot::Mutex;

use crate::error::{BoundKind, Error, Result};
use crate::mapping_table::{MappingTable, retire_chain};
use crate::node::{
    LeafSearchResult, child_for_key, consolidate, inner_needs_merge, leaf_needs_merge,
    leaf_needs_split, leaf_size, logical_inner_entries, logical_leaf_entries, search_leaf_chain,
};
use crate::options::BwTreeOptions;
use crate::overflow::OverflowStore;
use crate::page::{
    BaseNode, DeltaKind, InnerBase, LeafBase, NodeHeader, PageState, Payload, Pid, Value, NULL_PID,
};

/// In-memory Bw-Tree.
pub(crate) struct Tree {
    mapping_table: Arc<MappingTable>,
    overflow: Arc<OverflowStore>,
    options: BwTreeOptions,
    /// Serializes structural modifications in the first version.
    smo_lock: Mutex<()>,
}

impl Tree {
    /// Create a new tree backed by `mapping_table`.
    pub fn new(
        mapping_table: Arc<MappingTable>,
        overflow: Arc<OverflowStore>,
        options: &BwTreeOptions,
    ) -> Self {
        Self {
            mapping_table,
            overflow,
            options: options.clone(),
            smo_lock: Mutex::new(()),
        }
    }

    /// Read a single key.
    pub fn get(&self, root_pid: Pid, key: &[u8]) -> Result<Option<Bytes>> {
        let guard = epoch::pin();
        let result = self.get_recursive(root_pid, key, &guard)?;
        drop(guard);
        Ok(result)
    }

    fn get_recursive(&self, pid: Pid, key: &[u8], guard: &epoch::Guard) -> Result<Option<Bytes>> {
        if pid == NULL_PID {
            return Ok(None);
        }
        let state_ptr = self
            .mapping_table
            .load(pid)
            .ok_or_else(|| Error::Corruption(format!("missing page {pid}")))?;
        let state = unsafe { &*state_ptr };

        // Handle remove redirect on a leaf that has been merged away.
        if let Payload::Delta(DeltaKind::Remove { left_sibling }) = &state.payload {
            return self.get_recursive(*left_sibling, key, guard);
        }

        if state.header.depth == 0 {
            // Leaf: follow side link if the key is past the high key.
            if !state.header.high_key.is_empty() && key >= state.header.high_key.as_ref() {
                if let Some(right) = state.header.right_sibling {
                    return self.get_recursive(right, key, guard);
                }
            }
            match search_leaf_chain(state, key) {
                LeafSearchResult::Found(value) => Ok(Some(self.resolve_value(value)?)),
                LeafSearchResult::Deleted => Ok(None),
                LeafSearchResult::NeedBase => Err(Error::Corruption(format!(
                    "leaf {pid} chain missing base"
                ))),
            }
        } else {
            let entries = logical_inner_entries(state);
            let child = child_for_key(&entries, key);
            if child == NULL_PID {
                return Err(Error::Corruption(format!("null child in inner node {pid}")));
            }
            self.get_recursive(child, key, guard)
        }
    }

    /// Insert or overwrite a key/value pair.
    pub fn insert(&self, root_pid: Pid, key: &[u8], value: &[u8], lsn: u64) -> Result<Pid> {
        self.check_key_size(key)?;
        let value = self.encode_value(value)?;
        let guard = epoch::pin();
        let mut current_root = root_pid;
        loop {
            match self.try_insert(current_root, key, value.clone(), lsn, &guard) {
                Ok(new_root) => {
                    current_root = new_root;
                    break;
                }
                Err(Error::Conflict(_)) => {
                    // CAS failed; retry from root.
                    continue;
                }
                Err(e) => {
                    drop(guard);
                    return Err(e);
                }
            }
        }
        drop(guard);
        Ok(current_root)
    }

    fn try_insert(
        &self,
        root_pid: Pid,
        key: &[u8],
        value: Value,
        lsn: u64,
        guard: &epoch::Guard,
    ) -> Result<Pid> {
        let path = self.find_leaf_path(root_pid, key, guard)?;
        let (leaf_pid, leaf_ptr) = *path.last().ok_or_else(|| {
            Error::Corruption("empty path during insert".into())
        })?;
        let leaf_state = unsafe { &*leaf_ptr };

        // Build insert delta.
        let new_item_count = leaf_state.header.item_count + 1;
        let new_chain_len = leaf_state.header.delta_chain_length + 1;
        let new_header = NodeHeader {
            item_count: new_item_count,
            delta_chain_length: new_chain_len,
            ..leaf_state.header.clone()
        };
        let delta = Box::into_raw(Box::new(PageState::new(
            new_header,
            Payload::Delta(DeltaKind::Insert {
                key: Bytes::copy_from_slice(key),
                value,
            }),
            leaf_ptr,
            lsn,
        )));

        match self
            .mapping_table
            .compare_exchange(leaf_pid, leaf_ptr, delta)
        {
            Ok(_) => {
                let mut new_root = root_pid;
                if unsafe { (*delta).header.delta_chain_length }
                    > self.options.max_delta_chain_len_leaf as u32
                {
                    new_root = self.maybe_consolidate_path(&path, guard)?;
                }
                if leaf_needs_split(&logical_leaf_entries(unsafe { &*delta }), &self.options) {
                    new_root = self.split_leaf(new_root, leaf_pid, key, guard)?;
                }
                Ok(new_root)
            }
            Err(_) => {
                let _ = unsafe { Box::from_raw(delta) };
                Err(Error::Conflict("CAS failed on insert".into()))
            }
        }
    }

    /// Delete a key.
    pub fn delete(&self, root_pid: Pid, key: &[u8], lsn: u64) -> Result<Pid> {
        let guard = epoch::pin();
        let mut current_root = root_pid;
        loop {
            match self.try_delete(current_root, key, lsn, &guard) {
                Ok(new_root) => {
                    current_root = new_root;
                    break;
                }
                Err(Error::Conflict(_)) => continue,
                Err(e) => {
                    drop(guard);
                    return Err(e);
                }
            }
        }
        drop(guard);
        Ok(current_root)
    }

    fn try_delete(
        &self,
        root_pid: Pid,
        key: &[u8],
        lsn: u64,
        guard: &epoch::Guard,
    ) -> Result<Pid> {
        let path = self.find_leaf_path(root_pid, key, guard)?;
        let (leaf_pid, leaf_ptr) = *path.last().ok_or_else(|| {
            Error::Corruption("empty path during delete".into())
        })?;
        let leaf_state = unsafe { &*leaf_ptr };

        let new_header = NodeHeader {
            item_count: leaf_state.header.item_count,
            delta_chain_length: leaf_state.header.delta_chain_length + 1,
            ..leaf_state.header.clone()
        };
        let delta = Box::into_raw(Box::new(PageState::new(
            new_header,
            Payload::Delta(DeltaKind::Delete {
                key: Bytes::copy_from_slice(key),
            }),
            leaf_ptr,
            lsn,
        )));

        match self
            .mapping_table
            .compare_exchange(leaf_pid, leaf_ptr, delta)
        {
            Ok(_) => {
                let mut new_root = root_pid;
                if unsafe { (*delta).header.delta_chain_length }
                    > self.options.max_delta_chain_len_leaf as u32
                {
                    new_root = self.maybe_consolidate_path(&path, guard)?;
                }
                if leaf_needs_merge(&logical_leaf_entries(unsafe { &*delta }), &self.options) {
                    new_root = self.merge_leaf(new_root, leaf_pid, key, guard)?;
                }
                Ok(new_root)
            }
            Err(_) => {
                let _ = unsafe { Box::from_raw(delta) };
                Err(Error::Conflict("CAS failed on delete".into()))
            }
        }
    }

    fn find_leaf_path(
        &self,
        root_pid: Pid,
        key: &[u8],
        _guard: &epoch::Guard,
    ) -> Result<Vec<(Pid, *mut PageState)>> {
        let mut path = Vec::new();
        let mut current_pid = root_pid;
        loop {
            let state_ptr = self
                .mapping_table
                .load(current_pid)
                .ok_or_else(|| Error::Corruption(format!("missing page {current_pid}")))?;
            let state = unsafe { &*state_ptr };
            path.push((current_pid, state_ptr));
            if state.header.depth == 0 {
                return Ok(path);
            }
            let entries = logical_inner_entries(state);
            let child = child_for_key(&entries, key);
            if child == NULL_PID {
                return Err(Error::Corruption(format!(
                    "null child in inner node {current_pid}"
                )));
            }
            current_pid = child;
        }
    }

    fn maybe_consolidate_path(
        &self,
        path: &[(Pid, *mut PageState)],
        guard: &epoch::Guard,
    ) -> Result<Pid> {
        let mut new_root = path[0].0;
        for &(pid, ptr) in path.iter().rev() {
            let state = unsafe { &*ptr };
            let threshold = if state.header.depth == 0 {
                self.options.max_delta_chain_len_leaf
            } else {
                self.options.max_delta_chain_len_inner
            } as u32;
            if state.header.delta_chain_length > threshold {
                new_root = self.consolidate_node(new_root, pid, ptr, guard)?;
            }
        }
        Ok(new_root)
    }

    fn consolidate_node(
        &self,
        root_pid: Pid,
        pid: Pid,
        state_ptr: *mut PageState,
        guard: &epoch::Guard,
    ) -> Result<Pid> {
        let state = unsafe { &*state_ptr };
        let (new_state, _old_head) = consolidate(state, &self.options);
        let new_ptr = Box::into_raw(Box::new(new_state));
        if self.mapping_table.compare_exchange(pid, state_ptr, new_ptr).is_err() {
            let _ = unsafe { Box::from_raw(new_ptr) };
            return Ok(root_pid);
        }
        unsafe { retire_chain(state_ptr, guard) };
        Ok(root_pid)
    }

    fn split_leaf(&self, root_pid: Pid, leaf_pid: Pid, key: &[u8], guard: &epoch::Guard) -> Result<Pid> {
        let _lock = self.smo_lock.lock();
        // Re-traverse to get the current leaf state.
        let path = self.find_leaf_path(root_pid, key, guard)?;
        let (current_leaf_pid, current_leaf_ptr) = *path.last().ok_or_else(|| {
            Error::Corruption("empty path during split".into())
        })?;
        if current_leaf_pid != leaf_pid {
            return Ok(root_pid);
        }

        // Consolidate the leaf so we have a clean base to split.
        let _ = self.consolidate_node(root_pid, current_leaf_pid, current_leaf_ptr, guard)?;
        let leaf_ptr = self
            .mapping_table
            .load(current_leaf_pid)
            .ok_or_else(|| Error::Corruption(format!("missing leaf {current_leaf_pid}")))?;
        let leaf_state = unsafe { &*leaf_ptr };
        let entries = match &leaf_state.payload {
            Payload::Base(BaseNode::Leaf(leaf)) => leaf.entries.clone(),
            _ => return Ok(root_pid),
        };
        if !leaf_needs_split(&entries, &self.options) {
            return Ok(root_pid);
        }

        let split_point = find_split_point(&entries, &self.options)?;
        let right_entries = entries.split_at(split_point).1.to_vec();
        let left_entries = entries.split_at(split_point).0.to_vec();
        let separator = right_entries[0].0.clone();
        let right_pid = self.mapping_table.allocate_pid();
        let old_right_sibling = leaf_state.header.right_sibling;

        // Build right sibling base node.
        let right_header = NodeHeader {
            low_key: separator.clone(),
            high_key: leaf_state.header.high_key.clone(),
            right_sibling: old_right_sibling,
            item_count: right_entries.len() as u32,
            depth: 0,
            delta_chain_length: 0,
        };
        let right_state = PageState::new(
            right_header,
            Payload::Base(BaseNode::Leaf(LeafBase {
                entries: right_entries,
            })),
            std::ptr::null_mut(),
            leaf_state.lsn,
        );
        let right_ptr = Box::into_raw(Box::new(right_state));
        self.mapping_table.store(right_pid, right_ptr);

        // Build split delta on the left leaf.
        let split_delta_header = NodeHeader {
            low_key: leaf_state.header.low_key.clone(),
            high_key: separator.clone(),
            right_sibling: Some(right_pid),
            item_count: left_entries.len() as u32,
            depth: 0,
            delta_chain_length: leaf_state.header.delta_chain_length + 1,
        };
        let split_delta = PageState::new(
            split_delta_header,
            Payload::Delta(DeltaKind::Split {
                split_key: separator.clone(),
                new_right_sibling: right_pid,
            }),
            leaf_ptr,
            leaf_state.lsn,
        );
        let split_delta_ptr = Box::into_raw(Box::new(split_delta));
        if self
            .mapping_table
            .compare_exchange(current_leaf_pid, leaf_ptr, split_delta_ptr)
            .is_err()
        {
            let _ = unsafe { Box::from_raw(right_ptr) };
            let _ = unsafe { Box::from_raw(split_delta_ptr) };
            self.mapping_table.free_pid(right_pid);
            return Ok(root_pid);
        }

        // Update parent with a separator delta.
        if path.len() == 1 {
            // Root split: create a new root inner node.
            let new_root_pid = self.mapping_table.allocate_pid();
            let root_header = NodeHeader {
                low_key: Bytes::new(),
                high_key: Bytes::new(),
                right_sibling: None,
                item_count: 1,
                depth: 1,
                delta_chain_length: 0,
            };
            let root_state = PageState::new(
                root_header,
                Payload::Base(BaseNode::Inner(InnerBase {
                    entries: vec![(separator, right_pid)],
                    leftmost_child: current_leaf_pid,
                })),
                std::ptr::null_mut(),
                leaf_state.lsn,
            );
            let root_ptr = Box::into_raw(Box::new(root_state));
            self.mapping_table.store(new_root_pid, root_ptr);
            return Ok(new_root_pid);
        }

        let (parent_pid, parent_ptr) = path[path.len() - 2];
        self.post_separator(parent_pid, parent_ptr, separator, right_pid, guard)?;
        Ok(root_pid)
    }

    fn post_separator(
        &self,
        parent_pid: Pid,
        parent_ptr: *mut PageState,
        separator: Bytes,
        right_pid: Pid,
        _guard: &epoch::Guard,
    ) -> Result<()> {
        let parent_state = unsafe { &*parent_ptr };
        let new_header = NodeHeader {
            item_count: parent_state.header.item_count + 1,
            delta_chain_length: parent_state.header.delta_chain_length + 1,
            ..parent_state.header.clone()
        };
        let entries = logical_inner_entries(parent_state);
        let next_sep = entries
            .iter()
            .find(|(k, _)| k.as_ref() > separator.as_ref())
            .map(|(k, _)| k.clone())
            .unwrap_or_default();
        let sep_delta = PageState::new(
            new_header,
            Payload::Delta(DeltaKind::Separator {
                separator_key: separator,
                new_child: right_pid,
                next_separator_key: next_sep,
            }),
            parent_ptr,
            parent_state.lsn,
        );
        let sep_delta_ptr = Box::into_raw(Box::new(sep_delta));
        if self
            .mapping_table
            .compare_exchange(parent_pid, parent_ptr, sep_delta_ptr)
            .is_err()
        {
            let _ = unsafe { Box::from_raw(sep_delta_ptr) };
            // Parent changed; the split is already visible via side link, so
            // another thread must have helped. This is safe.
        }
        Ok(())
    }

    fn merge_leaf(&self, root_pid: Pid, leaf_pid: Pid, key: &[u8], guard: &epoch::Guard) -> Result<Pid> {
        let _lock = self.smo_lock.lock();
        // Find the leaf and its left sibling.
        let path = self.find_leaf_path(root_pid, key, guard)?;
        // Re-find the specific leaf in the path or re-traverse.
        let (current_leaf_pid, current_leaf_ptr) = *path.last().ok_or_else(|| {
            Error::Corruption("empty path during merge".into())
        })?;
        if current_leaf_pid != leaf_pid {
            return Ok(root_pid);
        }
        let leaf_state = unsafe { &*current_leaf_ptr };
        if leaf_state.header.depth != 0 {
            return Ok(root_pid);
        }

        // Consolidate the leaf.
        let _ = self.consolidate_node(root_pid, current_leaf_pid, current_leaf_ptr, guard)?;
        let leaf_ptr = self
            .mapping_table
            .load(current_leaf_pid)
            .ok_or_else(|| Error::Corruption(format!("missing leaf {current_leaf_pid}")))?;
        let leaf_state = unsafe { &*leaf_ptr };
        let leaf_entries = match &leaf_state.payload {
            Payload::Base(BaseNode::Leaf(leaf)) => leaf.entries.clone(),
            _ => return Ok(root_pid),
        };
        if !leaf_needs_merge(&leaf_entries, &self.options) {
            return Ok(root_pid);
        }

        // Find left sibling via parent.
        if path.len() < 2 {
            // Leaf is the root; cannot merge.
            return Ok(root_pid);
        }
        let (parent_pid, parent_ptr) = path[path.len() - 2];
        let _parent_entries = logical_inner_entries(unsafe { &*parent_ptr });
        let (left_sibling_pid, left_sibling_ptr) = match self.find_left_sibling(
            parent_pid,
            parent_ptr,
            current_leaf_pid,
            guard,
        )? {
            Some(p) => p,
            None => return Ok(root_pid),
        };
        let left_state = unsafe { &*left_sibling_ptr };
        let left_entries = match &left_state.payload {
            Payload::Base(BaseNode::Leaf(leaf)) => leaf.entries.clone(),
            _ => logical_leaf_entries(left_state),
        };

        let merged_entries: Vec<(Bytes, Value)> = left_entries
            .into_iter()
            .chain(leaf_entries)
            .collect();
        if leaf_size(&merged_entries) > self.options.node_size_threshold() {
            // Combined node would overflow; redistribution is not implemented.
            return Ok(root_pid);
        }

        // Post Remove delta on the right (current) leaf, pointing to left sibling.
        let remove_delta_header = NodeHeader {
            low_key: leaf_state.header.low_key.clone(),
            high_key: leaf_state.header.high_key.clone(),
            right_sibling: leaf_state.header.right_sibling,
            item_count: leaf_state.header.item_count,
            depth: 0,
            delta_chain_length: leaf_state.header.delta_chain_length + 1,
        };
        let remove_delta = PageState::new(
            remove_delta_header,
            Payload::Delta(DeltaKind::Remove {
                left_sibling: left_sibling_pid,
            }),
            leaf_ptr,
            leaf_state.lsn,
        );
        let remove_delta_ptr = Box::into_raw(Box::new(remove_delta));
        if self
            .mapping_table
            .compare_exchange(current_leaf_pid, leaf_ptr, remove_delta_ptr)
            .is_err()
        {
            let _ = unsafe { Box::from_raw(remove_delta_ptr) };
            return Ok(root_pid);
        }

        // Post Merge delta on the left sibling.
        let merge_delta_header = NodeHeader {
            low_key: left_state.header.low_key.clone(),
            high_key: leaf_state.header.high_key.clone(),
            right_sibling: leaf_state.header.right_sibling,
            item_count: (merged_entries.len()) as u32,
            depth: 0,
            delta_chain_length: left_state.header.delta_chain_length + 1,
        };
        let merge_delta = PageState::new(
            merge_delta_header,
            Payload::Delta(DeltaKind::Merge {
                merge_key: leaf_state.header.high_key.clone(),
                merged_node: current_leaf_pid,
            }),
            left_sibling_ptr,
            left_state.lsn,
        );
        let merge_delta_ptr = Box::into_raw(Box::new(merge_delta));
        if self
            .mapping_table
            .compare_exchange(left_sibling_pid, left_sibling_ptr, merge_delta_ptr)
            .is_err()
        {
            let _ = unsafe { Box::from_raw(merge_delta_ptr) };
            return Ok(root_pid);
        }

        // Update parent to remove the right leaf's separator.
        let parent_state = unsafe { &*parent_ptr };
        let parent_entries = logical_inner_entries(parent_state);
        let sep_to_remove = parent_entries
            .iter()
            .find(|(_, pid)| *pid == current_leaf_pid)
            .map(|(k, _)| k.clone())
            .unwrap_or_default();
        let next_sep = parent_entries
            .iter()
            .find(|(k, _)| k.as_ref() > sep_to_remove.as_ref())
            .map(|(k, _)| k.clone())
            .unwrap_or_default();
        let sep_delta = PageState::new(
            NodeHeader {
                item_count: parent_state.header.item_count.saturating_sub(1),
                delta_chain_length: parent_state.header.delta_chain_length + 1,
                ..parent_state.header.clone()
            },
            Payload::Delta(DeltaKind::Separator {
                separator_key: sep_to_remove,
                new_child: left_sibling_pid,
                next_separator_key: next_sep,
            }),
            parent_ptr,
            parent_state.lsn,
        );
        let sep_delta_ptr = Box::into_raw(Box::new(sep_delta));
        if self
            .mapping_table
            .compare_exchange(parent_pid, parent_ptr, sep_delta_ptr)
            .is_err()
        {
            let _ = unsafe { Box::from_raw(sep_delta_ptr) };
        }

        // Mark right leaf PID as free (delayed by epoch).
        self.mapping_table.store(current_leaf_pid, std::ptr::null_mut());
        unsafe { retire_chain(remove_delta_ptr, guard) };

        // If parent underflows, recursively merge up.
        let parent_after = logical_inner_entries(unsafe { &*self.mapping_table.load(parent_pid).unwrap() });
        if inner_needs_merge(&parent_after, &self.options) && path.len() > 2 {
            return self.merge_inner(root_pid, parent_pid, guard);
        }
        Ok(root_pid)
    }

    fn find_left_sibling(
        &self,
        _parent_pid: Pid,
        parent_ptr: *mut PageState,
        child_pid: Pid,
        _guard: &epoch::Guard,
    ) -> Result<Option<(Pid, *mut PageState)>> {
        let parent_state = unsafe { &*parent_ptr };
        let entries = logical_inner_entries(parent_state);
        let mut prev: Option<Pid> = None;
        for (_, pid) in &entries {
            if *pid == child_pid {
                return Ok(prev.and_then(|p| self.mapping_table.load(p).map(|ptr| (p, ptr))));
            }
            prev = Some(*pid);
        }
        Ok(None)
    }

    fn merge_inner(&self, root_pid: Pid, inner_pid: Pid, guard: &epoch::Guard) -> Result<Pid> {
        // Simplified: do not recursively merge inner nodes in the first version.
        // This can leave the tree slightly unbalanced but still correct.
        let _ = (root_pid, inner_pid, guard);
        Ok(root_pid)
    }

    fn resolve_value(&self, value: Value) -> Result<Bytes> {
        match value {
            Value::Inline(bytes) => Ok(bytes),
            Value::Overflow(offset) => self.overflow.read(offset),
        }
    }

    fn encode_value(&self, value: &[u8]) -> Result<Value> {
        if value.len() > self.options.max_inline_value_size {
            let offset = self.overflow.write(value)?;
            Ok(Value::Overflow(offset))
        } else {
            Ok(Value::Inline(Bytes::copy_from_slice(value)))
        }
    }

    fn check_key_size(&self, key: &[u8]) -> Result<()> {
        // A key must fit in a single-entry leaf alongside an inline empty value.
        let max_key_size = self
            .options
            .node_size_threshold()
            .saturating_sub(8 + 2 + 1 + 8 + 8)
            .max(1);
        if key.len() > max_key_size {
            return Err(Error::OutOfBounds {
                kind: BoundKind::Key,
                limit: max_key_size,
                got: key.len(),
            });
        }
        Ok(())
    }
}

fn find_split_point(entries: &[(Bytes, Value)], options: &BwTreeOptions) -> Result<usize> {
    let mut split_point = entries.len() / 2;
    split_point = split_point.max(1).min(entries.len().saturating_sub(1));
    loop {
        let left = &entries[..split_point];
        let right = &entries[split_point..];
        if leaf_size(left) <= options.node_size_threshold()
            && leaf_size(right) <= options.node_size_threshold()
        {
            return Ok(split_point);
        }
        if split_point + 1 < entries.len() {
            split_point += 1;
        } else if split_point > 1 {
            split_point -= 1;
        } else {
            return Err(Error::Corruption(
                "cannot split leaf: single entry does not fit".into(),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tree(page_size: usize) -> (Tree, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let overflow = Arc::new(OverflowStore::open(dir.path()).unwrap());
        let table = Arc::new(MappingTable::new());
        let options = BwTreeOptions {
            page_size,
            max_inline_value_size: page_size / 4,
            max_delta_chain_len_leaf: 4,
            max_delta_chain_len_inner: 4,
            min_fill_percent: 50,
        };
        let tree = Tree::new(table, overflow, &options);
        (tree, dir)
    }

    #[test]
    fn insert_and_get() {
        let (tree, _dir) = make_tree(512);
        let mut root = NULL_PID;
        root = tree.insert(root, b"a", b"1", 1).unwrap();
        root = tree.insert(root, b"b", b"2", 2).unwrap();
        assert_eq!(tree.get(root, b"a").unwrap(), Some(Bytes::from_static(b"1")));
        assert_eq!(tree.get(root, b"b").unwrap(), Some(Bytes::from_static(b"2")));
        assert_eq!(tree.get(root, b"c").unwrap(), None);
    }

    #[test]
    fn delete_key() {
        let (tree, _dir) = make_tree(512);
        let mut root = NULL_PID;
        root = tree.insert(root, b"a", b"1", 1).unwrap();
        root = tree.insert(root, b"b", b"2", 2).unwrap();
        root = tree.delete(root, b"a", 3).unwrap();
        assert_eq!(tree.get(root, b"a").unwrap(), None);
        assert_eq!(tree.get(root, b"b").unwrap(), Some(Bytes::from_static(b"2")));
    }

    #[test]
    fn many_inserts_trigger_split() {
        let (tree, _dir) = make_tree(512);
        let mut root = NULL_PID;
        for i in 0..100u8 {
            root = tree.insert(root, &[i], &[i + 100], i as u64 + 1).unwrap();
        }
        for i in 0..100u8 {
            assert_eq!(
                tree.get(root, &[i]).unwrap(),
                Some(Bytes::from(vec![i + 100]))
            );
        }
    }
}
