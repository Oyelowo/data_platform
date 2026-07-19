//! Adaptive Radix Trie node layouts.
//!
//! This module defines the four adaptive inner-node layouts (`Node4`, `Node16`,
//! `Node48`, `Node256`) and the terminal `Leaf`. Children are stored as
//! `AtomicPtr<Node>` so that optimistic readers can load child pointers
//! atomically while writers mutate the node under its version latch.

pub mod leaf;
pub mod node16;
pub mod node256;
pub mod node4;
pub mod node48;

pub use leaf::Leaf;
pub use node4::Node4;
pub use node16::Node16;
pub use node48::Node48;
pub use node256::Node256;

use std::sync::Arc;

use crate::node::Node;

/// Common operations implemented by all inner node layouts.
///
/// All mutation methods assume the caller holds the node's write latch.
#[allow(dead_code)]
pub(crate) trait InnerNode {
    /// The compressed prefix shared by all children.
    fn prefix(&self) -> &[u8];

    /// Number of populated children.
    fn child_count(&self) -> usize;

    /// Return the raw child pointer for `byte`, or null if absent.
    fn find_child(&self, byte: u8) -> *mut Node;

    /// Add a new child. Returns `Err(child)` if the node is full.
    fn add_child(&self, byte: u8, child: Arc<Node>) -> Result<(), Arc<Node>>;

    /// Replace an existing child and return the previous child, if any.
    fn replace_child(&self, byte: u8, child: Arc<Node>) -> Option<Arc<Node>>;

    /// Remove a child and return it, if present.
    fn remove_child(&self, byte: u8) -> Option<Arc<Node>>;

    /// Grow to the next larger node type because the node is full.
    fn grow(&self) -> Node;

    /// Shrink to the next smaller node type if below the threshold.
    /// Returns `None` if no shrink is needed.
    fn shrink(&self) -> Option<Node>;

    /// Return the smallest (partial-key, child-pointer) pair.
    fn first_child(&self) -> Option<(u8, *mut Node)>;

    /// Return the next child after `after_byte` in ascending partial-key order.
    fn next_child(&self, after_byte: u8) -> Option<(u8, *mut Node)>;
}
