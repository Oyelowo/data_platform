//! Adaptive Radix Trie node types.

use std::sync::Arc;

/// The maximum height of an ART tree for a key of length `L` is `L + 1`.
pub const MAX_KEY_LEN: usize = 2048;

/// Discriminant for the four adaptive node layouts.
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
///
/// Internal nodes store a compressed prefix and a variable number of children
/// depending on their fan-out. Leaves store the full key and its associated value.
#[derive(Debug)]
pub enum Node {
    /// Up to 4 children.
    Node4 {
        /// Compressed path prefix shared by all children.
        prefix: Box<[u8]>,
        /// Partial keys for the children (sorted ascending).
        keys: [u8; 4],
        /// Child pointers; `None` where no child exists.
        children: [Option<Arc<Node>>; 4],
        /// Number of populated children.
        count: u8,
    },
    /// Up to 16 children.
    Node16 {
        prefix: Box<[u8]>,
        keys: [u8; 16],
        children: [Option<Arc<Node>>; 16],
        count: u8,
    },
    /// Up to 48 children.
    Node48 {
        prefix: Box<[u8]>,
        /// Index from partial key (0..256) to 1-based index in `children`.
        /// A value of `0` means no child for that partial key.
        key_index: [u8; 256],
        children: [Option<Arc<Node>>; 48],
        count: u8,
    },
    /// Up to 256 children.
    Node256 {
        prefix: Box<[u8]>,
        children: [Option<Arc<Node>>; 256],
        count: u16,
    },
    /// Leaf containing the complete key and value.
    Leaf {
        key: Box<[u8]>,
        value: Box<[u8]>,
    },
}

impl Node {
    /// Return the node type discriminant.
    pub fn node_type(&self) -> NodeType {
        match self {
            Node::Node4 { .. } => NodeType::Node4,
            Node::Node16 { .. } => NodeType::Node16,
            Node::Node48 { .. } => NodeType::Node48,
            Node::Node256 { .. } => NodeType::Node256,
            Node::Leaf { .. } => NodeType::Leaf,
        }
    }
}
