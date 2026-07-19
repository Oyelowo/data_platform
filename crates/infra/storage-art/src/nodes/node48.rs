//! ART `Node48` layout: 17..=48 children using a 256-byte key index.

use std::sync::atomic::{AtomicPtr, AtomicU8, Ordering};
use std::sync::Arc;

use crate::keys::truncate_prefix;
use crate::latch::VersionLatch;
use crate::node::Node;
use crate::nodes::node16::Node16;
use crate::nodes::node256::Node256;
use crate::nodes::InnerNode;

/// Inner node that can hold up to forty-eight children.
#[derive(Debug)]
pub struct Node48 {
    /// Version latch for optimistic lock coupling.
    pub latch: VersionLatch,
    /// Compressed path prefix.
    pub prefix: Box<[u8]>,
    /// Leaf stored at this node, if the prefix itself is a key.
    pub leaf: AtomicPtr<Node>,
    /// Maps partial key (0..256) to 1-based index in `children`. Zero means absent.
    pub key_index: [AtomicU8; 256],
    /// Child pointer storage. Unused slots are null.
    pub children: [AtomicPtr<Node>; 48],
    /// Number of valid children (0..=48).
    pub count: AtomicU8,
}

impl Node48 {
    /// Create an empty `Node48` with the given prefix.
    pub fn new(prefix: Box<[u8]>) -> Self {
        Self {
            latch: VersionLatch::new(),
            prefix: truncate_prefix(&prefix).into(),
            leaf: AtomicPtr::new(std::ptr::null_mut()),
            key_index: std::array::from_fn(|_| AtomicU8::new(0)),
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

    pub(crate) fn next_free_slot(&self) -> usize {
        self.children
            .iter()
            .position(|c| c.load(Ordering::Relaxed).is_null())
            .expect("Node48 has a free slot")
    }

    /// Return the raw pointer for `byte`, or null if absent.
    pub fn child_ptr(&self, byte: u8) -> *mut Node {
        let idx = self.key_index[byte as usize].load(Ordering::Acquire);
        if idx == 0 {
            return std::ptr::null_mut();
        }
        self.children[(idx - 1) as usize].load(Ordering::Acquire)
    }

    /// Insert a new child. Panics if full.
    pub fn insert(&self, byte: u8, child: Arc<Node>) {
        assert!(!self.is_full(), "Node48 is full");
        let slot = self.next_free_slot();
        self.key_index[byte as usize].store((slot + 1) as u8, Ordering::Relaxed);
        self.children[slot].store(crate::node::arc_to_ptr(child), Ordering::Relaxed);
        self.count.store(self.count() + 1, Ordering::Relaxed);
    }

    /// Replace an existing child.
    pub fn replace(&self, byte: u8, child: Arc<Node>) -> Option<Arc<Node>> {
        let idx = self.key_index[byte as usize].load(Ordering::Relaxed);
        if idx == 0 {
            return None;
        }
        let slot = (idx - 1) as usize;
        let old = self.children[slot].swap(crate::node::arc_to_ptr(child), Ordering::Relaxed);
        unsafe { crate::node::take_ptr(old) }
    }

    /// Remove a child.
    pub fn remove(&self, byte: u8) -> Option<Arc<Node>> {
        let idx = self.key_index[byte as usize].load(Ordering::Relaxed);
        if idx == 0 {
            return None;
        }
        let slot = (idx - 1) as usize;
        let old = self.children[slot].swap(std::ptr::null_mut(), Ordering::Relaxed);
        self.key_index[byte as usize].store(0, Ordering::Relaxed);
        self.count.store(self.count() - 1, Ordering::Relaxed);
        unsafe { crate::node::take_ptr(old) }
    }

    /// True if no more children can be added.
    pub fn is_full(&self) -> bool {
        self.count() >= 48
    }

    /// True if the node should shrink to `Node16`.
    pub fn should_shrink(&self) -> bool {
        self.count() <= 16
    }

    /// Grow into a `Node256`, incrementing reference counts of all children.
    pub fn grow(&self) -> Node {
        let new = Node256::new(self.prefix.clone());
        let leaf_ptr = self.leaf.load(Ordering::Relaxed);
        if !leaf_ptr.is_null() {
            unsafe { Arc::increment_strong_count(leaf_ptr) };
        }
        new.leaf.store(leaf_ptr, Ordering::Relaxed);
        for byte in 0..=255u8 {
            let idx = self.key_index[byte as usize].load(Ordering::Relaxed);
            if idx != 0 {
                let slot = (idx - 1) as usize;
                let ptr = self.children[slot].load(Ordering::Relaxed);
                if !ptr.is_null() {
                    unsafe { Arc::increment_strong_count(ptr) };
                }
                new.children[byte as usize].store(ptr, Ordering::Relaxed);
            }
        }
        new.count.store(self.count() as u16, Ordering::Relaxed);
        Node::Node256(new)
    }

    /// Shrink into a `Node16`, incrementing reference counts of all children.
    pub fn shrink(&self) -> Option<Node> {
        if !self.should_shrink() {
            return None;
        }
        let new = Node16::new(self.prefix.clone());
        let leaf_ptr = self.leaf.load(Ordering::Relaxed);
        if !leaf_ptr.is_null() {
            unsafe { Arc::increment_strong_count(leaf_ptr) };
        }
        new.leaf.store(leaf_ptr, Ordering::Relaxed);
        let mut count = 0u8;
        for byte in 0..=255u8 {
            let idx = self.key_index[byte as usize].load(Ordering::Relaxed);
            if idx != 0 {
                let slot = (idx - 1) as usize;
                let ptr = self.children[slot].load(Ordering::Relaxed);
                if !ptr.is_null() {
                    unsafe { Arc::increment_strong_count(ptr) };
                }
                new.keys[count as usize].store(byte, Ordering::Relaxed);
                new.children[count as usize].store(ptr, Ordering::Relaxed);
                count += 1;
            }
        }
        new.count.store(count, Ordering::Relaxed);
        Some(Node::Node16(new))
    }

    /// Clone this node with a different prefix, incrementing child/leaf refcounts.
    pub fn clone_with_prefix(&self, prefix: Box<[u8]>) -> Self {
        let new = Self::new(prefix);
        let leaf_ptr = self.leaf.load(Ordering::Relaxed);
        if !leaf_ptr.is_null() {
            unsafe { Arc::increment_strong_count(leaf_ptr) };
        }
        new.leaf.store(leaf_ptr, Ordering::Relaxed);
        let mut count = 0u8;
        for byte in 0..=255u8 {
            let idx = self.key_index[byte as usize].load(Ordering::Relaxed);
            if idx != 0 {
                let slot = (idx - 1) as usize;
                let ptr = self.children[slot].load(Ordering::Relaxed);
                if !ptr.is_null() {
                    unsafe { Arc::increment_strong_count(ptr) };
                }
                new.key_index[byte as usize].store(count + 1, Ordering::Relaxed);
                new.children[count as usize].store(ptr, Ordering::Relaxed);
                count += 1;
            }
        }
        new.count.store(count, Ordering::Relaxed);
        new
    }
}

impl InnerNode for Node48 {
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
        for byte in 0..=255u8 {
            let idx = self.key_index[byte as usize].load(Ordering::Acquire);
            if idx != 0 {
                let slot = (idx - 1) as usize;
                let ptr = self.children[slot].load(Ordering::Acquire);
                return Some((byte, ptr));
            }
        }
        None
    }

    fn next_child(&self, after_byte: u8) -> Option<(u8, *mut Node)> {
        for byte in (after_byte as u16 + 1)..=255u16 {
            let byte = byte as u8;
            let idx = self.key_index[byte as usize].load(Ordering::Acquire);
            if idx != 0 {
                let slot = (idx - 1) as usize;
                let ptr = self.children[slot].load(Ordering::Acquire);
                return Some((byte, ptr));
            }
        }
        None
    }
}
