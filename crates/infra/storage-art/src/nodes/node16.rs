//! ART `Node16` layout: 5..=16 children stored in sorted key order.

use std::sync::Arc;
use std::sync::atomic::{AtomicPtr, AtomicU8, Ordering};

use crate::keys::truncate_prefix;
use crate::latch::VersionLatch;
use crate::node::Node;
use crate::nodes::InnerNode;
use crate::nodes::node4::Node4;
use crate::nodes::node48::Node48;

/// Inner node that can hold up to sixteen children.
#[derive(Debug)]
pub struct Node16 {
    /// Version latch for optimistic lock coupling.
    pub latch: VersionLatch,
    /// Compressed path prefix.
    pub prefix: Box<[u8]>,
    /// Leaf stored at this node, if the prefix itself is a key.
    pub leaf: AtomicPtr<Node>,
    /// Partial keys, sorted ascending. Only the first `count` entries are valid.
    pub keys: [AtomicU8; 16],
    /// Child pointers.
    pub children: [AtomicPtr<Node>; 16],
    /// Number of valid children (0..=16).
    pub count: AtomicU8,
}

impl Node16 {
    /// Create an empty `Node16` with the given prefix.
    pub fn new(prefix: Box<[u8]>) -> Self {
        Self {
            latch: VersionLatch::new(),
            prefix: truncate_prefix(&prefix).into(),
            leaf: AtomicPtr::new(std::ptr::null_mut()),
            keys: std::array::from_fn(|_| AtomicU8::new(0)),
            children: std::array::from_fn(|_| AtomicPtr::new(std::ptr::null_mut())),
            count: AtomicU8::new(0),
        }
    }

    /// Read the leaf pointer stored at this node, returning an `Arc` clone.
    pub fn leaf(&self) -> Option<Arc<Node>> {
        let ptr = self.leaf.load(Ordering::Acquire);
        unsafe { crate::node::ptr_to_arc(ptr) }
    }

    /// Store a leaf at this node, returning any previous leaf.
    pub fn set_leaf(&self, leaf: Arc<Node>) -> Option<Arc<Node>> {
        let ptr = crate::node::arc_to_ptr(leaf);
        let old = self.leaf.swap(ptr, Ordering::Relaxed);
        unsafe { crate::node::take_ptr(old) }
    }

    /// Remove the leaf stored at this node, returning it.
    pub fn take_leaf(&self) -> Option<Arc<Node>> {
        let old = self.leaf.swap(std::ptr::null_mut(), Ordering::Relaxed);
        unsafe { crate::node::take_ptr(old) }
    }

    fn count(&self) -> u8 {
        self.count.load(Ordering::Relaxed)
    }

    /// Return the index of `byte` if it exists.
    pub fn find(&self, byte: u8) -> Option<usize> {
        let count = self.count();
        (0..count as usize).find(|&i| self.keys[i].load(Ordering::Relaxed) == byte)
    }

    /// Return the raw pointer for `byte`, or null if absent.
    pub fn child_ptr(&self, byte: u8) -> *mut Node {
        self.find(byte)
            .map(|i| self.children[i].load(Ordering::Acquire))
            .unwrap_or(std::ptr::null_mut())
    }

    /// Insert a new child. Panics if full.
    pub fn insert(&self, byte: u8, child: Arc<Node>) {
        assert!(!self.is_full(), "Node16 is full");
        let count = self.count();
        let pos = (0..count as usize)
            .find(|&i| self.keys[i].load(Ordering::Relaxed) > byte)
            .unwrap_or(count as usize);
        for i in (pos..count as usize).rev() {
            let k = self.keys[i].load(Ordering::Relaxed);
            self.keys[i + 1].store(k, Ordering::Relaxed);
            let ptr = self.children[i].load(Ordering::Relaxed);
            self.children[i + 1].store(ptr, Ordering::Relaxed);
        }
        self.keys[pos].store(byte, Ordering::Relaxed);
        self.children[pos].store(crate::node::arc_to_ptr(child), Ordering::Relaxed);
        self.count.store(count + 1, Ordering::Relaxed);
    }

    /// Replace an existing child.
    pub fn replace(&self, byte: u8, child: Arc<Node>) -> Option<Arc<Node>> {
        self.find(byte).and_then(|i| {
            let old = self.children[i].swap(crate::node::arc_to_ptr(child), Ordering::Relaxed);
            unsafe { crate::node::take_ptr(old) }
        })
    }

    /// Remove a child.
    pub fn remove(&self, byte: u8) -> Option<Arc<Node>> {
        self.find(byte).and_then(|pos| {
            let count = self.count();
            let old = self.children[pos].swap(std::ptr::null_mut(), Ordering::Relaxed);
            for i in pos..(count as usize - 1) {
                let k = self.keys[i + 1].load(Ordering::Relaxed);
                self.keys[i].store(k, Ordering::Relaxed);
                let ptr = self.children[i + 1].load(Ordering::Relaxed);
                self.children[i].store(ptr, Ordering::Relaxed);
            }
            self.children[count as usize - 1].store(std::ptr::null_mut(), Ordering::Relaxed);
            self.keys[count as usize - 1].store(0, Ordering::Relaxed);
            self.count.store(count - 1, Ordering::Relaxed);
            unsafe { crate::node::take_ptr(old) }
        })
    }

    /// True if no more children can be added.
    pub fn is_full(&self) -> bool {
        self.count() >= 16
    }

    /// True if the node should shrink to `Node4`.
    pub fn should_shrink(&self) -> bool {
        self.count() <= 4
    }

    /// Grow into a `Node48`, incrementing reference counts of all children.
    pub fn grow(&self) -> Node {
        let new = Node48::new(self.prefix.clone());
        let leaf_ptr = self.leaf.load(Ordering::Relaxed);
        if !leaf_ptr.is_null() {
            unsafe { Arc::increment_strong_count(leaf_ptr) };
        }
        new.leaf.store(leaf_ptr, Ordering::Relaxed);
        let count = self.count();
        for i in 0..count as usize {
            let byte = self.keys[i].load(Ordering::Relaxed);
            let ptr = self.children[i].load(Ordering::Relaxed);
            if !ptr.is_null() {
                unsafe { Arc::increment_strong_count(ptr) };
            }
            let slot = new.next_free_slot();
            new.key_index[byte as usize].store((slot + 1) as u8, Ordering::Relaxed);
            new.children[slot].store(ptr, Ordering::Relaxed);
        }
        new.count.store(count, Ordering::Relaxed);
        Node::Node48(new)
    }

    /// Shrink into a `Node4`, incrementing reference counts of all children.
    pub fn shrink(&self) -> Option<Node> {
        if !self.should_shrink() {
            return None;
        }
        let new = Node4::new(self.prefix.clone());
        let leaf_ptr = self.leaf.load(Ordering::Relaxed);
        if !leaf_ptr.is_null() {
            unsafe { Arc::increment_strong_count(leaf_ptr) };
        }
        new.leaf.store(leaf_ptr, Ordering::Relaxed);
        let count = self.count();
        new.count.store(count, Ordering::Relaxed);
        for i in 0..count as usize {
            new.keys[i].store(self.keys[i].load(Ordering::Relaxed), Ordering::Relaxed);
            let ptr = self.children[i].load(Ordering::Relaxed);
            if !ptr.is_null() {
                unsafe { Arc::increment_strong_count(ptr) };
            }
            new.children[i].store(ptr, Ordering::Relaxed);
        }
        Some(Node::Node4(new))
    }

    /// Clone this node with a different prefix, incrementing child/leaf refcounts.
    pub fn clone_with_prefix(&self, prefix: Box<[u8]>) -> Self {
        let new = Self::new(prefix);
        let leaf_ptr = self.leaf.load(Ordering::Relaxed);
        if !leaf_ptr.is_null() {
            unsafe { Arc::increment_strong_count(leaf_ptr) };
        }
        new.leaf.store(leaf_ptr, Ordering::Relaxed);
        let count = self.count();
        new.count.store(count, Ordering::Relaxed);
        for i in 0..count as usize {
            new.keys[i].store(self.keys[i].load(Ordering::Relaxed), Ordering::Relaxed);
            let ptr = self.children[i].load(Ordering::Relaxed);
            if !ptr.is_null() {
                unsafe { Arc::increment_strong_count(ptr) };
            }
            new.children[i].store(ptr, Ordering::Relaxed);
        }
        new
    }
}

impl InnerNode for Node16 {
    fn prefix(&self) -> &[u8] {
        &self.prefix
    }

    fn child_count(&self) -> usize {
        self.count() as usize
    }

    fn find_child(&self, byte: u8) -> *mut Node {
        self.child_ptr(byte)
    }

    fn add_child(&self, byte: u8, child: Arc<Node>) -> Result<(), Arc<Node>> {
        if self.is_full() {
            Err(child)
        } else {
            self.insert(byte, child);
            Ok(())
        }
    }

    fn replace_child(&self, byte: u8, child: Arc<Node>) -> Option<Arc<Node>> {
        self.replace(byte, child)
    }

    fn remove_child(&self, byte: u8) -> Option<Arc<Node>> {
        self.remove(byte)
    }

    fn grow(&self) -> Node {
        self.grow()
    }

    fn shrink(&self) -> Option<Node> {
        self.shrink()
    }

    fn first_child(&self) -> Option<(u8, *mut Node)> {
        let count = self.count();
        if count == 0 {
            return None;
        }
        let byte = self.keys[0].load(Ordering::Acquire);
        let ptr = self.children[0].load(Ordering::Acquire);
        Some((byte, ptr))
    }

    fn next_child(&self, after_byte: u8) -> Option<(u8, *mut Node)> {
        let count = self.count();
        for i in 0..count as usize {
            let byte = self.keys[i].load(Ordering::Acquire);
            if byte > after_byte {
                let ptr = self.children[i].load(Ordering::Acquire);
                return Some((byte, ptr));
            }
        }
        None
    }
}
