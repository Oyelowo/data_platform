//! Adaptive Radix Trie node types.
//!
//! This module defines the public `Node` enum and `NodeType` discriminant. The
//! concrete layouts live in [`crate::nodes`]. Children are stored as
//! `AtomicPtr<Node>` backed by `Arc<Node>` reference counts so that readers can
//! load child pointers atomically while writers mutate under version latches.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::latch::VersionLatch;
use crate::nodes::{InnerNode, Leaf, Node16, Node256, Node4, Node48};

/// The maximum key length supported by the default configuration.
pub const MAX_KEY_LEN: usize = 2048;

/// Discriminant for the four adaptive node layouts plus the terminal leaf.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeType {
    /// Up to 4 children.
    Node4,
    /// Up to 16 children.
    Node16,
    /// Up to 48 children.
    Node48,
    /// Up to 256 children.
    Node256,
    /// Terminal leaf.
    Leaf,
}

/// A node in the Adaptive Radix Trie.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum Node {
    /// Up to 4 children.
    Node4(Node4),
    /// Up to 16 children.
    Node16(Node16),
    /// Up to 48 children.
    Node48(Node48),
    /// Up to 256 children.
    Node256(Node256),
    /// Terminal leaf.
    Leaf(Leaf),
}

impl Drop for Node {
    fn drop(&mut self) {
        // Recursively drop the Arc<Node> references held by each child slot and
        // the optional leaf stored at inner nodes. The Arc is stored as a raw
        // pointer in an AtomicPtr. We load each non-null pointer, convert it
        // back to Arc, and drop it.
        macro_rules! drop_children {
            ($children:expr) => {
                for child in $children.iter() {
                    let ptr = child.load(Ordering::Relaxed);
                    if !ptr.is_null() {
                        unsafe { drop_ptr(ptr) };
                    }
                }
            };
        }
        macro_rules! drop_leaf {
            ($leaf:expr) => {
                let ptr = $leaf.load(Ordering::Relaxed);
                if !ptr.is_null() {
                    unsafe { drop_ptr(ptr) };
                }
            };
        }
        match self {
            Node::Node4(n) => {
                drop_leaf!(n.leaf);
                drop_children!(n.children);
            }
            Node::Node16(n) => {
                drop_leaf!(n.leaf);
                drop_children!(n.children);
            }
            Node::Node48(n) => {
                drop_leaf!(n.leaf);
                drop_children!(n.children);
            }
            Node::Node256(n) => {
                drop_leaf!(n.leaf);
                drop_children!(n.children);
            }
            Node::Leaf(_) => {}
        }
    }
}

impl Node {
    /// Return the node type discriminant.
    pub fn node_type(&self) -> NodeType {
        match self {
            Node::Node4(_) => NodeType::Node4,
            Node::Node16(_) => NodeType::Node16,
            Node::Node48(_) => NodeType::Node48,
            Node::Node256(_) => NodeType::Node256,
            Node::Leaf(_) => NodeType::Leaf,
        }
    }

    /// Reference to the node's version latch, or a default for leaves.
    ///
    /// Leaves do not participate in lock coupling; this default latch is only
    /// provided so that generic traversal code can record a version. Leaf
    /// mutations (replacing a leaf in its parent) are protected by the parent
    /// latch.
    pub fn latch(&self) -> &VersionLatch {
        match self {
            Node::Node4(n) => &n.latch,
            Node::Node16(n) => &n.latch,
            Node::Node48(n) => &n.latch,
            Node::Node256(n) => &n.latch,
            Node::Leaf(_) => {
                // Leaves are immutable; a shared latch is never contended.
                static LEAF_LATCH: VersionLatch = VersionLatch::new();
                &LEAF_LATCH
            }
        }
    }

    /// Compressed prefix for inner nodes, empty for leaves.
    pub fn prefix(&self) -> &[u8] {
        match self {
            Node::Node4(n) => &n.prefix,
            Node::Node16(n) => &n.prefix,
            Node::Node48(n) => &n.prefix,
            Node::Node256(n) => &n.prefix,
            Node::Leaf(_) => &[],
        }
    }

    /// True if this node is a leaf.
    pub fn is_leaf(&self) -> bool {
        matches!(self, Node::Leaf(_))
    }

    /// Access the leaf contents, if this node is a leaf.
    pub fn as_leaf(&self) -> Option<&Leaf> {
        match self {
            Node::Leaf(leaf) => Some(leaf),
            _ => None,
        }
    }

    /// Return the leaf stored at this inner node, if any.
    pub fn inner_leaf(&self) -> Option<Arc<Node>> {
        match self {
            Node::Node4(n) => n.leaf(),
            Node::Node16(n) => n.leaf(),
            Node::Node48(n) => n.leaf(),
            Node::Node256(n) => n.leaf(),
            Node::Leaf(_) => None,
        }
    }

    /// Set the leaf stored at this inner node, returning the previous leaf.
    pub fn set_inner_leaf(&self, leaf: Arc<Node>) -> Option<Arc<Node>> {
        match self {
            Node::Node4(n) => n.set_leaf(leaf),
            Node::Node16(n) => n.set_leaf(leaf),
            Node::Node48(n) => n.set_leaf(leaf),
            Node::Node256(n) => n.set_leaf(leaf),
            Node::Leaf(_) => None,
        }
    }

    /// Take the leaf stored at this inner node, if any.
    pub fn take_inner_leaf(&self) -> Option<Arc<Node>> {
        match self {
            Node::Node4(n) => n.take_leaf(),
            Node::Node16(n) => n.take_leaf(),
            Node::Node48(n) => n.take_leaf(),
            Node::Node256(n) => n.take_leaf(),
            Node::Leaf(_) => None,
        }
    }

    /// True if this inner node cannot accept any more children.
    pub fn is_full(&self) -> bool {
        match self {
            Node::Node4(n) => n.is_full(),
            Node::Node16(n) => n.is_full(),
            Node::Node48(n) => n.is_full(),
            Node::Node256(n) => n.is_full(),
            Node::Leaf(_) => true,
        }
    }

    /// Number of children for inner nodes; zero for leaves.
    pub fn child_count(&self) -> usize {
        match self {
            Node::Node4(n) => n.child_count(),
            Node::Node16(n) => n.child_count(),
            Node::Node48(n) => n.child_count(),
            Node::Node256(n) => n.child_count(),
            Node::Leaf(_) => 0,
        }
    }

    /// Return the raw child pointer for `byte`, or null if absent.
    ///
    /// # Safety
    ///
    /// The returned pointer is owned by an `Arc<Node>`. Callers that need to
    /// keep it alive must convert it with [`ptr_to_arc`] before releasing any
    /// lock that protects the parent.
    pub fn find_child(&self, byte: u8) -> *mut Node {
        match self {
            Node::Node4(n) => n.find_child(byte),
            Node::Node16(n) => n.find_child(byte),
            Node::Node48(n) => n.find_child(byte),
            Node::Node256(n) => n.find_child(byte),
            Node::Leaf(_) => std::ptr::null_mut(),
        }
    }

    /// Return the smallest (partial-key, child-pointer) pair.
    pub fn first_child(&self) -> Option<(u8, *mut Node)> {
        match self {
            Node::Node4(n) => n.first_child(),
            Node::Node16(n) => n.first_child(),
            Node::Node48(n) => n.first_child(),
            Node::Node256(n) => n.first_child(),
            Node::Leaf(_) => None,
        }
    }

    /// Return the next child after `after_byte` in ascending partial-key order.
    pub fn next_child(&self, after_byte: u8) -> Option<(u8, *mut Node)> {
        match self {
            Node::Node4(n) => n.next_child(after_byte),
            Node::Node16(n) => n.next_child(after_byte),
            Node::Node48(n) => n.next_child(after_byte),
            Node::Node256(n) => n.next_child(after_byte),
            Node::Leaf(_) => None,
        }
    }

    /// Create a new leaf wrapped in an `Arc`.
    pub fn new_leaf(key: Box<[u8]>, value: Box<[u8]>) -> Arc<Self> {
        Arc::new(Node::Leaf(Leaf::new(key, value)))
    }

    /// Create a new inner node of the smallest type with the given prefix.
    pub fn new_inner(prefix: Box<[u8]>) -> Arc<Self> {
        Arc::new(Node::Node4(Node4::new(prefix)))
    }

    /// Add a child under the parent write latch. Returns `Err(child)` if full.
    pub fn add_child(&self, byte: u8, child: Arc<Node>) -> Result<(), Arc<Node>> {
        match self {
            Node::Node4(n) => n.add_child(byte, child),
            Node::Node16(n) => n.add_child(byte, child),
            Node::Node48(n) => n.add_child(byte, child),
            Node::Node256(n) => n.add_child(byte, child),
            Node::Leaf(_) => Err(child),
        }
    }

    /// Replace an existing child under the parent write latch.
    pub fn replace_child(&self, byte: u8, child: Arc<Node>) -> Option<Arc<Node>> {
        match self {
            Node::Node4(n) => n.replace_child(byte, child),
            Node::Node16(n) => n.replace_child(byte, child),
            Node::Node48(n) => n.replace_child(byte, child),
            Node::Node256(n) => n.replace_child(byte, child),
            Node::Leaf(_) => None,
        }
    }

    /// Remove a child under the write latch.
    pub fn remove_child(&self, byte: u8) -> Option<Arc<Node>> {
        match self {
            Node::Node4(n) => n.remove_child(byte),
            Node::Node16(n) => n.remove_child(byte),
            Node::Node48(n) => n.remove_child(byte),
            Node::Node256(n) => n.remove_child(byte),
            Node::Leaf(_) => None,
        }
    }

    /// Grow this node to the next larger layout, returning a new `Node` value.
    ///
    /// The new node increments the reference count of each child/leaf, so it is
    /// safe to call while the original node remains referenced.
    pub fn grow(&self) -> Node {
        match self {
            Node::Node4(n) => n.grow(),
            Node::Node16(n) => n.grow(),
            Node::Node48(n) => n.grow(),
            Node::Node256(n) => n.grow(),
            Node::Leaf(_) => panic!("cannot grow a leaf"),
        }
    }

    /// Shrink this node if it has fallen below its layout threshold.
    pub fn shrink(&self) -> Option<Node> {
        match self {
            Node::Node4(n) => n.shrink(),
            Node::Node16(n) => n.shrink(),
            Node::Node48(n) => n.shrink(),
            Node::Node256(n) => n.shrink(),
            Node::Leaf(_) => None,
        }
    }

    /// Create a deep copy of this node with a different prefix.
    ///
    /// The new node shares the same children/leaf via incremented reference
    /// counts; it does not own exclusive copies.
    pub fn clone_with_prefix(&self, prefix: Box<[u8]>) -> Node {
        match self {
            Node::Node4(n) => Node::Node4(n.clone_with_prefix(prefix)),
            Node::Node16(n) => Node::Node16(n.clone_with_prefix(prefix)),
            Node::Node48(n) => Node::Node48(n.clone_with_prefix(prefix)),
            Node::Node256(n) => Node::Node256(n.clone_with_prefix(prefix)),
            Node::Leaf(leaf) => Node::Leaf(Leaf::new(leaf.key.clone(), leaf.value.clone())),
        }
    }

}

/// Convert an `Arc<Node>` into a raw pointer for storage in an `AtomicPtr`.
pub(crate) fn arc_to_ptr(arc: Arc<Node>) -> *mut Node {
    Arc::into_raw(arc) as *mut Node
}

/// Increment the reference count of `ptr` and return an `Arc<Node>`.
///
/// # Safety
///
/// `ptr` must be a pointer returned by [`arc_to_ptr`] that is still valid
/// (i.e., the `Arc` has not been dropped yet). This function is safe for null
/// pointers; it returns `None`.
pub(crate) unsafe fn ptr_to_arc(ptr: *mut Node) -> Option<Arc<Node>> {
    if ptr.is_null() {
        return None;
    }
    unsafe { Arc::increment_strong_count(ptr) };
    Some(unsafe { Arc::from_raw(ptr) })
}

/// Take ownership of an `Arc<Node>` reference held as a raw pointer.
///
/// This is the counterpart to [`arc_to_ptr`] for removing a child/leaf from a
/// node: the atomic slot no longer owns the reference, so the caller takes it
/// without incrementing the strong count.
///
/// # Safety
///
/// `ptr` must be a pointer returned by [`arc_to_ptr`]. This function is safe for
/// null pointers; it returns `None`.
pub(crate) unsafe fn take_ptr(ptr: *mut Node) -> Option<Arc<Node>> {
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { Arc::from_raw(ptr) })
    }
}

/// Drop an owned `Arc<Node>` reference held as a raw pointer.
///
/// # Safety
///
/// `ptr` must be a non-null pointer returned by [`arc_to_ptr`] that is not
/// referenced by any other `Arc` clone.
pub(crate) unsafe fn drop_ptr(ptr: *mut Node) {
    debug_assert!(!ptr.is_null());
    unsafe { drop(Arc::from_raw(ptr)) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_type_discriminants() {
        let leaf = Node::new_leaf(b"k".to_vec().into(), b"v".to_vec().into());
        assert_eq!(leaf.node_type(), NodeType::Leaf);
        let inner = Node::new_inner(b"p".to_vec().into());
        assert_eq!(inner.node_type(), NodeType::Node4);
    }

    #[test]
    fn leaf_prefix_is_empty() {
        let leaf = Node::new_leaf(b"k".to_vec().into(), b"v".to_vec().into());
        assert!(leaf.prefix().is_empty());
        assert!(leaf.as_leaf().is_some());
    }
}
