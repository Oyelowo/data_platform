//! ART `Node256` layout: 49..=256 children using a direct 256-element array.

use std::sync::atomic::{AtomicPtr, AtomicU16, Ordering};
use std::sync::Arc;

use crate::keys::truncate_prefix;
use crate::latch::VersionLatch;
use crate::node::Node;
use crate::nodes::node48::Node48;
use crate::nodes::InnerNode;

/// Inner node that can hold up to 256 children.
#[derive(Debug)]
pub struct Node256 {
    /// Version latch for optimistic lock coupling.
    pub latch: VersionLatch,
    /// Compressed path prefix.
    pub prefix: Box<[u8]>,
    /// Leaf stored at this node, if the prefix itself is a key.
    pub leaf: AtomicPtr<Node>,
    /// Direct child pointers indexed by partial key.
    pub children: [AtomicPtr<Node>; 256],
    /// Number of valid children (0..=256).
    pub count: AtomicU16,
}

impl Node256 {
    /// Create an empty `Node256` with the given prefix.
    pub fn new(prefix: Box<[u8]>) -> Self {
        Self {
            latch: VersionLatch::new(),
            prefix: truncate_prefix(&prefix).into(),
            leaf: AtomicPtr::new(std::ptr::null_mut()),
            children: std::array::from_fn(|_| AtomicPtr::new(std::ptr::null_mut())),
            count: AtomicU16::new(0),
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

    fn count(&self) -> u16 {
        self.count.load(Ordering::Relaxed)
    }

    /// Return the raw pointer for `byte`, or null if absent.
    pub fn child_ptr(&self, byte: u8) -> *mut Node {
        self.children[byte as usize].load(Ordering::Acquire)
    }

    /// Insert a new child.
    pub fn insert(&self, byte: u8, child: Arc<Node>) {
        let slot = &self.children[byte as usize];
        if slot.load(Ordering::Relaxed).is_null() {
            self.count.store(self.count() + 1, Ordering::Relaxed);
        }
        slot.store(crate::node::arc_to_ptr(child), Ordering::Relaxed);
    }

    /// Replace an existing child.
    pub fn replace(&self, byte: u8, child: Arc<Node>) -> Option<Arc<Node>> {
        let old = self.children[byte as usize].swap(crate::node::arc_to_ptr(child), Ordering::Relaxed);
        unsafe { crate::node::take_ptr(old) }
    }

    /// Remove a child.
    pub fn remove(&self, byte: u8) -> Option<Arc<Node>> {
        let old = self.children[byte as usize].swap(std::ptr::null_mut(), Ordering::Relaxed);
        if !old.is_null() {
            self.count.store(self.count() - 1, Ordering::Relaxed);
        }
        unsafe { crate::node::take_ptr(old) }
    }

    /// True if no more children can be added.
    pub fn is_full(&self) -> bool {
        self.count() >= 256
    }

    /// True if the node should shrink to `Node48`.
    pub fn should_shrink(&self) -> bool {
        self.count() <= 48
    }

    /// Shrink into a `Node48`, incrementing reference counts of all children.
    pub fn shrink(&self) -> Option<Node> {
        if !self.should_shrink() {
            return None;
        }
        let new = Node48::new(self.prefix.clone());
        let leaf_ptr = self.leaf.load(Ordering::Relaxed);
        if !leaf_ptr.is_null() {
            unsafe { Arc::increment_strong_count(leaf_ptr) };
        }
        new.leaf.store(leaf_ptr, Ordering::Relaxed);
        let mut count = 0u8;
        for byte in 0..=255u8 {
            let ptr = self.children[byte as usize].load(Ordering::Relaxed);
            if !ptr.is_null() {
                unsafe { Arc::increment_strong_count(ptr) };
                let slot = new.next_free_slot();
                new.key_index[byte as usize].store((slot + 1) as u8, Ordering::Relaxed);
                new.children[slot].store(ptr, Ordering::Relaxed);
                count += 1;
            }
        }
        new.count.store(count, Ordering::Relaxed);
        Some(Node::Node48(new))
    }

    /// Clone this node with a different prefix, incrementing child/leaf refcounts.
    pub fn clone_with_prefix(&self, prefix: Box<[u8]>) -> Self {
        let new = Self::new(prefix);
        let leaf_ptr = self.leaf.load(Ordering::Relaxed);
        if !leaf_ptr.is_null() {
            unsafe { Arc::increment_strong_count(leaf_ptr) };
        }
        new.leaf.store(leaf_ptr, Ordering::Relaxed);
        let mut count = 0u16;
        for byte in 0..=255u8 {
            let ptr = self.children[byte as usize].load(Ordering::Relaxed);
            if !ptr.is_null() {
                unsafe { Arc::increment_strong_count(ptr) };
                new.children[byte as usize].store(ptr, Ordering::Relaxed);
                count += 1;
            }
        }
        new.count.store(count, Ordering::Relaxed);
        new
    }
}

impl InnerNode for Node256 {
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
        // Node256 is the largest layout; it cannot grow further.
        Node::Node256(Node256::new(self.prefix.clone()))
    }

    fn shrink(&self) -> Option<Node> {
        self.shrink()
    }

    fn first_child(&self) -> Option<(u8, *mut Node)> {
        for byte in 0..=255u8 {
            let ptr = self.children[byte as usize].load(Ordering::Acquire);
            if !ptr.is_null() {
                return Some((byte, ptr));
            }
        }
        None
    }

    fn next_child(&self, after_byte: u8) -> Option<(u8, *mut Node)> {
        for byte in (after_byte as u16 + 1)..=255u16 {
            let byte = byte as u8;
            let ptr = self.children[byte as usize].load(Ordering::Acquire);
            if !ptr.is_null() {
                return Some((byte, ptr));
            }
        }
        None
    }
}
