//! Page, node, and delta-record types for the Bw-Tree.

use bytes::Bytes;

/// Logical page identifier.
pub type Pid = u64;

/// Sentinel PID meaning "no page".
pub const NULL_PID: Pid = 0;

/// Header maintained on every element of a delta chain so that searches can
/// navigate without replaying the whole chain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeHeader {
    /// Smallest key that may be stored in this logical node.
    pub low_key: Bytes,
    /// Smallest key of the right sibling.
    pub high_key: Bytes,
    /// PID of the right sibling on the same level.
    pub right_sibling: Option<Pid>,
    /// Number of data items (leaf) or separators (inner) in the logical node.
    pub item_count: u32,
    /// Distance from the leaf level (0 for leaves).
    pub depth: u32,
    /// Number of delta records currently in the chain.
    pub delta_chain_length: u32,
}

impl Default for NodeHeader {
    fn default() -> Self {
        Self {
            low_key: Bytes::new(),
            high_key: Bytes::new(),
            right_sibling: None,
            item_count: 0,
            depth: 0,
            delta_chain_length: 0,
        }
    }
}

/// In-memory representation of a page. A page is a node in a delta chain: it
/// is either a base node or a delta record, and it points to the older state
/// via `next`.
pub struct PageState {
    /// Header describing the logical node at this point in the chain.
    pub header: NodeHeader,
    /// Payload of this chain element.
    pub payload: Payload,
    /// Physical pointer to the next older element in the chain.
    pub next: *mut PageState,
    /// LSN of the operation that produced this chain element.
    pub lsn: u64,
}

impl PageState {
    /// Create a new chain element.
    pub fn new(header: NodeHeader, payload: Payload, next: *mut PageState, lsn: u64) -> Self {
        Self {
            header,
            payload,
            next,
            lsn,
        }
    }
}

impl Drop for PageState {
    fn drop(&mut self) {
        if !self.next.is_null() {
            // Recursively drop the rest of the chain. This is safe because
            // `next` is only dropped once: when this node is reclaimed by the
            // epoch collector after no reader can reach it.
            let next = self.next;
            self.next = std::ptr::null_mut();
            unsafe {
                let _ = Box::from_raw(next);
            }
        }
    }
}

/// The payload of a [`PageState`].
pub enum Payload {
    /// A base node containing the sorted entries of a leaf or inner node.
    Base(BaseNode),
    /// A delta record describing a modification to the logical node.
    Delta(DeltaKind),
}

/// A base node: either a leaf or an inner node.
pub enum BaseNode {
    /// Leaf base node.
    Leaf(LeafBase),
    /// Inner base node.
    Inner(InnerBase),
}

/// Leaf base node containing sorted key/value entries.
pub struct LeafBase {
    /// Sorted `(key, value)` entries.
    pub entries: Vec<(Bytes, Value)>,
}

/// Inner base node containing sorted separator keys and child PIDs.
pub struct InnerBase {
    /// Sorted separator entries. `entries[0]` is the first separator *after*
    /// the leftmost child.
    pub entries: Vec<(Bytes, Pid)>,
    /// The leftmost child.
    pub leftmost_child: Pid,
}

/// Value stored in a leaf entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    /// Inline value bytes.
    Inline(Bytes),
    /// Head offset of an overflow value chain.
    Overflow(u64),
}

impl Value {
    /// Return the serialized size of this value for size accounting.
    pub fn serialized_size(&self) -> usize {
        match self {
            Value::Inline(bytes) => 1 + 8 + bytes.len(),
            Value::Overflow(_) => 1 + 8 + 8,
        }
    }
}

/// Delta record kinds. Deltas are prepended to a chain to install a state
/// change with a single CAS.
///
/// Several variants are not constructed in v1 (splits rebuild base nodes,
/// merges are disabled) but are kept for the next version.
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeltaKind {
    /// Insert or overwrite a key in a leaf.
    Insert {
        /// Key to insert.
        key: Bytes,
        /// Value to insert.
        value: Value,
    },
    /// Delete a key from a leaf.
    Delete {
        /// Key to delete.
        key: Bytes,
    },
    /// Split a node: keys >= `split_key` belong to the new right sibling.
    #[allow(dead_code)]
    Split {
        /// First key that belongs to the right sibling.
        split_key: Bytes,
        /// PID of the new right sibling.
        new_right_sibling: Pid,
    },
    /// Merge the right sibling into this node.
    Merge {
        /// High key of the merged right sibling.
        merge_key: Bytes,
        /// PID of the merged right sibling (used for navigation).
        merged_node: Pid,
    },
    /// Remove this node: it has been merged into its left sibling.
    Remove {
        /// PID of the left sibling that now owns this node's entries.
        left_sibling: Pid,
    },
    /// Update an inner node separator.
    Separator {
        /// Separator key for the new child.
        separator_key: Bytes,
        /// PID of the new child.
        new_child: Pid,
        /// Next separator key after `new_child` in the old base node.
        next_separator_key: Bytes,
    },
    /// Abort marker used to serialize structural modifications.
    Abort,
}

// Safety: `PageState` contains raw pointers, but they are only dereferenced
// while the current thread is pinned in an epoch, and retired state is
// reclaimed only after the epoch has advanced.
unsafe impl Send for PageState {}
unsafe impl Sync for PageState {}
