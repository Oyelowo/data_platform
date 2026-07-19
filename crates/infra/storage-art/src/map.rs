//! Adaptive Radix Trie map implementation.
//!
//! `ArtMap` is a concurrent in-memory ordered byte-key → byte-value map using
//! Optimistic Lock Coupling (OLC). Reads are lock-free optimistic traversals
//! that restart on version changes; writes use lock coupling (parent latch held
//! while installing or replacing a child pointer).

use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
use std::sync::Arc;

use bytes::Bytes;

use crate::error::{Error, Result};
use crate::keys::{common_prefix_len, match_prefix, truncate_prefix};
use crate::node::{arc_to_ptr, drop_ptr, ptr_to_arc, Node};
use crate::nodes::node4::Node4;
use crate::nodes::InnerNode;
use crate::options::ArtMapOptions;
use crate::snapshot;

/// An in-memory Adaptive Radix Trie mapping byte keys to byte values.
#[derive(Debug)]
pub struct ArtMap {
    /// Shared root pointer. Clones of the map share this atomic so that writes
    /// from one clone are visible to all clones.
    root: Arc<AtomicPtr<Node>>,
    options: ArtMapOptions,
    len: AtomicUsize,
}

// Safety: `ArtMap` owns the root pointer and all nodes are reference-counted
// (`Arc<Node>`). Send/Sync are inherited from the atomic root pointer.
unsafe impl Send for ArtMap {}
unsafe impl Sync for ArtMap {}

impl Clone for ArtMap {
    fn clone(&self) -> Self {
        Self {
            root: Arc::clone(&self.root),
            options: self.options.clone(),
            len: AtomicUsize::new(self.len.load(Ordering::Relaxed)),
        }
    }
}

impl Drop for ArtMap {
    fn drop(&mut self) {
        // Only free the tree when the last clone is dropped.
        if let Some(root) = Arc::get_mut(&mut self.root) {
            let ptr = root.swap(std::ptr::null_mut(), Ordering::Acquire);
            if !ptr.is_null() {
                unsafe {
                    drop(Arc::from_raw(ptr));
                }
            }
        }
    }
}

/// A raw write lock guard that keeps the locked node alive via `Arc<Node>`.
struct RawWriteGuard {
    node: Arc<Node>,
}

impl RawWriteGuard {
    fn lock(node: Arc<Node>) -> Self {
        node.latch().write_lock();
        Self { node }
    }

    fn node(&self) -> &Node {
        &self.node
    }

    fn arc(&self) -> &Arc<Node> {
        &self.node
    }
}

impl Drop for RawWriteGuard {
    fn drop(&mut self) {
        unsafe { self.node.latch().write_unlock() };
    }
}

impl ArtMap {
    /// Create a new empty `ArtMap` with the given options.
    pub fn new(options: ArtMapOptions) -> Self {
        Self {
            root: Arc::new(AtomicPtr::new(std::ptr::null_mut())),
            options,
            len: AtomicUsize::new(0),
        }
    }

    /// Return the current raw root pointer. Used by metrics/snapshot code that
    /// already holds a stable view of the map.
    pub(crate) fn root_ptr(&self) -> *mut Node {
        self.root.load(Ordering::Acquire)
    }

    fn check_key(&self, key: &[u8]) -> Result<()> {
        if key.len() > self.options.max_key_len {
            return Err(Error::KeyTooLong {
                len: key.len(),
                max: self.options.max_key_len,
            });
        }
        Ok(())
    }

    fn check_value(&self, value: &[u8]) -> Result<()> {
        if value.len() > self.options.max_value_len {
            return Err(Error::ValueTooLong {
                len: value.len(),
                max: self.options.max_value_len,
            });
        }
        Ok(())
    }

    /// Look up a key and return a clone of its value, if present.
    pub fn get(&self, key: &[u8]) -> Option<Bytes> {
        if key.len() > self.options.max_key_len {
            return None;
        }
        loop {
            let root_ptr = self.root.load(Ordering::Acquire);
            if root_ptr.is_null() {
                return None;
            }
            let root = unsafe { ptr_to_arc(root_ptr) }?;
            match self.get_from(root, key) {
                Ok(v) => return v,
                Err(()) => continue,
            }
        }
    }

    fn get_from(&self, root: Arc<Node>, key: &[u8]) -> Result<Option<Bytes>, ()> {
        let mut node = root;
        let mut depth = 0;
        loop {
            let v = node.latch().read_lock();
            if node.is_leaf() {
                let leaf = node.as_leaf().ok_or(())?;
                let ok = node.latch().read_unlock(v);
                if ok && leaf.key() == key {
                    return Ok(Some(Bytes::copy_from_slice(leaf.value())));
                }
                return if ok { Ok(None) } else { Err(()) };
            }

            let prefix = node.prefix();
            let prefix_len = prefix.len();
            if depth + prefix_len > key.len() || prefix != &key[depth..depth + prefix_len] {
                let ok = node.latch().read_unlock(v);
                return if ok { Ok(None) } else { Err(()) };
            }
            depth += prefix_len;

            if depth == key.len() {
                let leaf_opt = node.inner_leaf();
                let ok = node.latch().read_unlock(v);
                if !ok {
                    drop(leaf_opt);
                    return Err(());
                }
                return Ok(leaf_opt.and_then(|leaf_arc| {
                    leaf_arc.as_leaf().map(|l| Bytes::copy_from_slice(l.value()))
                }));
            }

            let byte = key[depth];
            depth += 1;
            let child_ptr = node.find_child(byte);
            let child = unsafe { ptr_to_arc(child_ptr) };
            let ok = node.latch().read_unlock(v);
            if !ok {
                drop(child);
                return Err(());
            }
            match child {
                Some(c) => node = c,
                None => return Ok(None),
            }
        }
    }

    /// Insert a key/value pair.
    ///
    /// Returns the previous value if the key already existed.
    pub fn insert(&self, key: &[u8], value: &[u8]) -> Result<Option<Bytes>> {
        self.check_key(key)?;
        self.check_value(value)?;
        if let Some(limit) = self.options.max_entries
            && self.len.load(Ordering::Relaxed) >= limit
        {
            return Err(Error::EntryLimitReached(limit));
        }
        let new_leaf = Node::new_leaf(key.into(), value.into());
        loop {
            let root_ptr = self.root.load(Ordering::Acquire);
            if root_ptr.is_null() {
                let leaf_ptr = arc_to_ptr(new_leaf.clone());
                if self
                    .root
                    .compare_exchange(root_ptr, leaf_ptr, Ordering::Release, Ordering::Relaxed)
                    .is_ok()
                {
                    self.len.fetch_add(1, Ordering::Relaxed);
                    return Ok(None);
                }
                unsafe { drop_ptr(leaf_ptr) };
                continue;
            }

            let root = match unsafe { ptr_to_arc(root_ptr) } {
                Some(r) => r,
                None => continue,
            };
            let root_guard = RawWriteGuard::lock(root);
            if !std::ptr::eq(self.root.load(Ordering::Acquire), Arc::as_ptr(root_guard.arc())) {
                drop(root_guard);
                continue;
            }

            match self.insert_locked(root_guard, key, new_leaf.clone()) {
                Ok((old, inserted)) => {
                    if inserted {
                        self.len.fetch_add(1, Ordering::Relaxed);
                    }
                    return Ok(old);
                }
                Err(()) => continue,
            }
        }
    }

    /// Insert under a locked root. On success the returned tuple is
    /// `(old_value, is_newly_inserted)`.
    fn insert_locked(
        &self,
        mut current: RawWriteGuard,
        key: &[u8],
        new_leaf: Arc<Node>,
    ) -> Result<(Option<Bytes>, bool), ()> {
        let mut prev: Option<RawWriteGuard> = None;
        let mut prev_byte: Option<u8> = None;
        let mut depth: usize = 0;

        loop {
            if prev.is_none()
                && !std::ptr::eq(self.root.load(Ordering::Acquire), Arc::as_ptr(current.arc()))
            {
                return Err(());
            }

            let node = current.node();

            // Handle a leaf root (or leaf reached via an optimistic restart).
            if node.is_leaf() {
                let leaf = node.as_leaf().ok_or(())?;
                if leaf.key() == key {
                    let old_value = Some(Bytes::copy_from_slice(leaf.value()));
                    self.install_child(prev, prev_byte, new_leaf, current)?;
                    return Ok((old_value, false));
                }
                let split = self.split_leaf(leaf, key, depth, new_leaf)?;
                self.install_child(prev, prev_byte, Arc::new(split), current)?;
                return Ok((None, true));
            }

            let prefix = node.prefix();
            let prefix_len = prefix.len();
            let cmp = match_prefix(prefix, key, depth);

            if cmp < prefix_len {
                let split = self.split_node(node, cmp, key, depth, new_leaf)?;
                self.install_child(prev, prev_byte, Arc::new(split), current)?;
                return Ok((None, true));
            }

            depth += prefix_len;

            if depth == key.len() {
                let old = node.inner_leaf();
                let old_value = old.as_ref().and_then(|arc| {
                    arc.as_leaf().map(|l| Bytes::copy_from_slice(l.value()))
                });
                let existed = old.is_some();
                node.set_inner_leaf(new_leaf);
                drop(old);
                drop(current);
                drop(prev);
                return Ok((old_value, !existed));
            }

            let byte = key[depth];
            depth += 1;
            let child_ptr = node.find_child(byte);

            if child_ptr.is_null() {
                if node.is_full() {
                    let grown = node.grow();
                    let grown_arc = Arc::new(grown);
                    grown_arc.add_child(byte, new_leaf).map_err(|_| ())?;
                    self.install_child(prev, prev_byte, grown_arc, current)?;
                } else {
                    node.add_child(byte, new_leaf).map_err(|_| ())?;
                    drop(current);
                    drop(prev);
                }
                return Ok((None, true));
            }

            let child = unsafe { ptr_to_arc(child_ptr).ok_or(())? };
            let child_guard = RawWriteGuard::lock(child);
            let child_node = child_guard.node();

            if child_node.is_leaf() {
                let leaf = child_node.as_leaf().ok_or(())?;
                if leaf.key() == key {
                    let old = current.node().replace_child(byte, new_leaf);
                    let old_value = old.as_ref().and_then(|arc| {
                        arc.as_leaf().map(|l| Bytes::copy_from_slice(l.value()))
                    });
                    drop(child_guard);
                    drop(old);
                    drop(current);
                    drop(prev);
                    return Ok((old_value, false));
                }
                let split = self.split_leaf(leaf, key, depth, new_leaf)?;
                let old = current.node().replace_child(byte, Arc::new(split));
                drop(child_guard);
                drop(old);
                drop(current);
                drop(prev);
                return Ok((None, true));
            }

            let child_prefix = child_node.prefix();
            let child_cmp = match_prefix(child_prefix, key, depth);
            if child_cmp < child_prefix.len() {
                let split = self.split_node(child_node, child_cmp, key, depth, new_leaf)?;
                let old = current.node().replace_child(byte, Arc::new(split));
                drop(child_guard);
                drop(old);
                drop(current);
                drop(prev);
                return Ok((None, true));
            }

            if depth + child_prefix.len() == key.len() {
                let old = child_node.inner_leaf();
                let old_value = old.as_ref().and_then(|arc| {
                    arc.as_leaf().map(|l| Bytes::copy_from_slice(l.value()))
                });
                let existed = old.is_some();
                child_node.set_inner_leaf(new_leaf);
                drop(child_guard);
                drop(current);
                drop(prev);
                return Ok((old_value, !existed));
            }

            // Descend: keep child locked, release previous parent.
            prev_byte = Some(byte);
            prev = Some(current);
            current = child_guard;
        }
    }

    /// Split `node` at `cmp` bytes into its prefix. The common prefix is
    /// `node.prefix[..cmp]`. A new inner node is returned with two children:
    /// one for the existing subtree and one for `new_leaf`.
    fn split_node(
        &self,
        node: &Node,
        cmp: usize,
        key: &[u8],
        depth: usize,
        new_leaf: Arc<Node>,
    ) -> Result<Node, ()> {
        let prefix = node.prefix();
        let common = &prefix[..cmp];
        let old_byte = prefix[cmp];
        let old_suffix = &prefix[cmp + 1..];
        let new_byte = key.get(depth + cmp).copied();

        let inner = Node4::new(common.into());
        let old_subtree = node.clone_with_prefix(old_suffix.into());
        inner.add_child(old_byte, Arc::new(old_subtree)).map_err(|_| ())?;

        if let Some(b) = new_byte {
            inner.add_child(b, new_leaf).map_err(|_| ())?;
        } else {
            inner.set_leaf(new_leaf);
        }
        Ok(Node::Node4(inner))
    }

    /// Split an existing leaf into a new inner node because `key` diverges.
    fn split_leaf(
        &self,
        leaf: &crate::nodes::Leaf,
        key: &[u8],
        depth: usize,
        new_leaf: Arc<Node>,
    ) -> Result<Node, ()> {
        let leaf_key = leaf.key();
        let common_len = common_prefix_len(leaf_key, key, depth);
        let common = truncate_prefix(&key[depth..depth + common_len]);
        let old_byte = leaf_key.get(depth + common_len).copied();
        let new_byte = key.get(depth + common_len).copied();

        let inner = Node4::new(common.into());
        if let Some(b) = old_byte {
            let old_leaf = Node::new_leaf(leaf.key().into(), leaf.value().into());
            inner.add_child(b, old_leaf).map_err(|_| ())?;
        } else {
            let old_leaf = Node::new_leaf(leaf.key().into(), leaf.value().into());
            inner.set_leaf(old_leaf);
        }

        if let Some(b) = new_byte {
            inner.add_child(b, new_leaf).map_err(|_| ())?;
        } else {
            inner.set_leaf(new_leaf);
        }
        Ok(Node::Node4(inner))
    }

    /// Install `new_child` in the parent (or as the root). `old_current` is the
    /// node being replaced and is dropped after the install attempt.
    fn install_child(
        &self,
        prev: Option<RawWriteGuard>,
        prev_byte: Option<u8>,
        new_child: Arc<Node>,
        old_current: RawWriteGuard,
    ) -> Result<(), ()> {
        match (prev, prev_byte) {
            (Some(prev_guard), Some(byte)) => {
                // `old_child` owns the tree reference that was removed from the
                // parent. Drop the guard first so `write_unlock` runs while the
                // node is still alive, then drop `old_child` to free it.
                let old_child = prev_guard.node().replace_child(byte, new_child).ok_or(())?;
                drop(old_current);
                drop(prev_guard);
                drop(old_child);
                Ok(())
            }
            (None, None) => {
                let new_ptr = arc_to_ptr(new_child);
                let old_ptr = Arc::as_ptr(old_current.arc()) as *mut Node;
                if self
                    .root
                    .compare_exchange(old_ptr, new_ptr, Ordering::Release, Ordering::Relaxed)
                    .is_ok()
                {
                    // Drop the guard first so `write_unlock` runs on a live
                    // node, then consume the tree reference that was removed
                    // from the root slot.
                    drop(old_current);
                    unsafe { drop_ptr(old_ptr) };
                    Ok(())
                } else {
                    unsafe { drop_ptr(new_ptr) };
                    Err(())
                }
            }
            _ => Err(()),
        }
    }

    /// Remove a key and return its value if it existed.
    pub fn remove(&self, key: &[u8]) -> Result<Option<Bytes>> {
        self.check_key(key)?;
        loop {
            let root_ptr = self.root.load(Ordering::Acquire);
            if root_ptr.is_null() {
                return Ok(None);
            }
            let root = match unsafe { ptr_to_arc(root_ptr) } {
                Some(r) => r,
                None => continue,
            };
            let root_guard = RawWriteGuard::lock(root);
            if !std::ptr::eq(self.root.load(Ordering::Acquire), Arc::as_ptr(root_guard.arc())) {
                drop(root_guard);
                continue;
            }
            match self.remove_locked(root_guard, key) {
                Ok((value, removed)) => {
                    if removed {
                        self.len.fetch_sub(1, Ordering::Relaxed);
                    }
                    return Ok(value);
                }
                Err(()) => continue,
            }
        }
    }

    fn remove_locked(
        &self,
        mut current: RawWriteGuard,
        key: &[u8],
    ) -> Result<(Option<Bytes>, bool), ()> {
        let mut prev: Option<RawWriteGuard> = None;
        let mut prev_byte: Option<u8> = None;
        let mut depth: usize = 0;

        loop {
            if prev.is_none()
                && !std::ptr::eq(self.root.load(Ordering::Acquire), Arc::as_ptr(current.arc()))
            {
                return Err(());
            }

            let node = current.node();

            if node.is_leaf() {
                let leaf = node.as_leaf().ok_or(())?;
                if leaf.key() == key {
                    let value = Some(Bytes::copy_from_slice(leaf.value()));
                    self.remove_empty_node(prev, prev_byte, current)?;
                    return Ok((value, true));
                }
                drop(current);
                drop(prev);
                return Ok((None, false));
            }

            let prefix = node.prefix();
            let prefix_len = prefix.len();
            if depth + prefix_len > key.len() || prefix != &key[depth..depth + prefix_len] {
                drop(current);
                drop(prev);
                return Ok((None, false));
            }
            depth += prefix_len;

            if depth == key.len() {
                let old = node.take_inner_leaf();
                let value = old.as_ref().and_then(|arc| {
                    arc.as_leaf().map(|l| Bytes::copy_from_slice(l.value()))
                });
                let removed = old.is_some();
                drop(old);

                // If the node has no children, remove it from its parent.
                if removed && node.child_count() == 0 {
                    self.remove_empty_node(prev, prev_byte, current)?;
                } else {
                    drop(current);
                    drop(prev);
                }
                return Ok((value, removed));
            }

            let byte = key[depth];
            depth += 1;
            let child_ptr = node.find_child(byte);
            if child_ptr.is_null() {
                drop(current);
                drop(prev);
                return Ok((None, false));
            }

            let child = unsafe { ptr_to_arc(child_ptr).ok_or(())? };
            let child_guard = RawWriteGuard::lock(child);
            let child_node = child_guard.node();

            if child_node.is_leaf() {
                let leaf = child_node.as_leaf().ok_or(())?;
                if leaf.key() == key {
                    let old = current.node().remove_child(byte);
                    let value = old.as_ref().and_then(|arc| {
                        arc.as_leaf().map(|l| Bytes::copy_from_slice(l.value()))
                    });
                    drop(child_guard);
                    drop(old);

                    // Shrink or compress the current node if appropriate.
                    if let Some(shrunk) = current.node().shrink() {
                        self.install_child(prev, prev_byte, Arc::new(shrunk), current)?;
                    } else {
                        drop(current);
                        drop(prev);
                    }
                    return Ok((value, true));
                }
                drop(child_guard);
                drop(current);
                drop(prev);
                return Ok((None, false));
            }

            // Descend.
            prev_byte = Some(byte);
            prev = Some(current);
            current = child_guard;
        }
    }

    /// Remove an empty inner node from its parent (or replace the root with null).
    fn remove_empty_node(
        &self,
        prev: Option<RawWriteGuard>,
        prev_byte: Option<u8>,
        current: RawWriteGuard,
    ) -> Result<(), ()> {
        match (prev, prev_byte) {
            (Some(prev_guard), Some(byte)) => {
                // `removed` owns the tree reference that was removed from the
                // parent. Drop the guard first so `write_unlock` runs while the
                // node is still alive, then drop `removed` to free it.
                let removed = prev_guard.node().remove_child(byte).ok_or(())?;
                if let Some(shrunk) = prev_guard.node().shrink() {
                    // Install shrunk node in its parent. We don't have that
                    // parent locked, so just leave the node as-is; it is still
                    // valid, only larger than necessary.
                    drop(shrunk);
                }
                drop(current);
                drop(prev_guard);
                drop(removed);
                Ok(())
            }
            (None, None) => {
                let old_ptr = Arc::as_ptr(current.arc()) as *mut Node;
                self.root
                    .compare_exchange(old_ptr, std::ptr::null_mut(), Ordering::Release, Ordering::Relaxed)
                    .map_err(|_| ())?;
                // Drop the guard first so `write_unlock` runs on a live node,
                // then consume the tree reference that was removed from the
                // root slot.
                drop(current);
                unsafe { drop_ptr(old_ptr) };
                Ok(())
            }
            _ => Err(()),
        }
    }

    /// Return the number of entries in the map.
    pub fn len(&self) -> usize {
        self.len.load(Ordering::Relaxed)
    }

    /// Return true if the map contains no entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Iterate over all keys that start with `prefix`.
    pub fn prefix(&self, prefix: &[u8]) -> crate::cursor::ArtCursor {
        crate::cursor::ArtCursor::prefix(self, prefix)
    }

    /// Iterate over keys in the half-open range `[start, end)`.
    pub fn range(&self, start: Option<&[u8]>, end: Option<&[u8]>) -> crate::cursor::ArtCursor {
        crate::cursor::ArtCursor::range(self, start, end)
    }

    /// Return an in-memory snapshot of the map.
    pub fn snapshot(&self) -> Result<Vec<u8>> {
        snapshot::encode(self)
    }

    /// Collect all key/value pairs into `out` in ascending lexicographic order.
    pub(crate) fn collect_entries(&self, out: &mut Vec<(Bytes, Bytes)>) {
        let root_ptr = self.root.load(Ordering::Acquire);
        if root_ptr.is_null() {
            return;
        }
        // We clone the root Arc so that the snapshot remains consistent even if
        // the root pointer changes during collection.
        if let Some(root) = unsafe { ptr_to_arc(root_ptr) } {
            Self::collect_from(&root, out);
        }
    }

    fn collect_from(node: &Arc<Node>, out: &mut Vec<(Bytes, Bytes)>) {
        if let Some(leaf) = node.as_leaf() {
            out.push((
                Bytes::copy_from_slice(leaf.key()),
                Bytes::copy_from_slice(leaf.value()),
            ));
            return;
        }
        if let Some(leaf) = node.inner_leaf()
            && let Some(leaf_ref) = leaf.as_leaf()
        {
            out.push((
                Bytes::copy_from_slice(leaf_ref.key()),
                Bytes::copy_from_slice(leaf_ref.value()),
            ));
        }
        let mut next_byte: Option<u8> = None;
        loop {
            let child_info = match next_byte {
                None => node.first_child(),
                Some(b) => node.next_child(b),
            };
            match child_info {
                Some((byte, ptr)) if !ptr.is_null() => {
                    if let Some(child) = unsafe { ptr_to_arc(ptr) } {
                        next_byte = Some(byte);
                        Self::collect_from(&child, out);
                    }
                }
                _ => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_map_is_empty() {
        let map = ArtMap::new(ArtMapOptions::default());
        assert!(map.is_empty());
        assert_eq!(map.len(), 0);
    }

    #[test]
    fn insert_and_get() {
        let map = ArtMap::new(ArtMapOptions::default());
        assert_eq!(map.insert(b"a", b"1").unwrap(), None);
        assert_eq!(map.get(b"a"), Some(Bytes::from_static(b"1")));
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn insert_overwrites() {
        let map = ArtMap::new(ArtMapOptions::default());
        map.insert(b"a", b"1").unwrap();
        assert_eq!(map.insert(b"a", b"2").unwrap(), Some(Bytes::from_static(b"1")));
        assert_eq!(map.get(b"a"), Some(Bytes::from_static(b"2")));
    }

    #[test]
    fn remove_existing() {
        let map = ArtMap::new(ArtMapOptions::default());
        map.insert(b"a", b"1").unwrap();
        assert_eq!(map.remove(b"a").unwrap(), Some(Bytes::from_static(b"1")));
        assert_eq!(map.get(b"a"), None);
    }

    #[test]
    fn remove_missing() {
        let map = ArtMap::new(ArtMapOptions::default());
        assert_eq!(map.remove(b"a").unwrap(), None);
    }

    #[test]
    fn empty_key() {
        let map = ArtMap::new(ArtMapOptions::default());
        map.insert(b"", b"empty").unwrap();
        assert_eq!(map.get(b""), Some(Bytes::from_static(b"empty")));
    }

    #[test]
    fn prefix_keys() {
        let map = ArtMap::new(ArtMapOptions::default());
        map.insert(b"a", b"1").unwrap();
        map.insert(b"ab", b"2").unwrap();
        assert_eq!(map.get(b"a"), Some(Bytes::from_static(b"1")));
        assert_eq!(map.get(b"ab"), Some(Bytes::from_static(b"2")));
    }

    #[test]
    fn long_common_prefix() {
        let map = ArtMap::new(ArtMapOptions::default());
        let a = b"aaaaaaaaaab";
        let b = b"aaaaaaaaaac";
        map.insert(a, b"1").unwrap();
        map.insert(b, b"2").unwrap();
        assert_eq!(map.get(a), Some(Bytes::from_static(b"1")));
        assert_eq!(map.get(b), Some(Bytes::from_static(b"2")));
    }
}
