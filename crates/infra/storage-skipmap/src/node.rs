//! Node layout and low-level helpers for the lock-free skip-map.

use crossbeam_epoch::{Atomic, Guard, Shared};
use std::sync::atomic::Ordering;

/// Tag bit used to mark a node as logically deleted.
pub const MARK_TAG: usize = 1;

/// A skip-map node.
///
/// The `key` and `value` are `Option`s so that the sentinel head node can hold
/// dummy `None` values without unsafe initialization. For regular entries both
/// are `Some`.
pub struct Node<K, V> {
    /// The entry key. `None` only for the sentinel head node.
    pub key: Option<K>,
    /// The entry value. `None` only for the sentinel head node.
    pub value: Option<V>,
    /// Tower of next pointers. Index 0 is the ground-level linked list.
    pub next: Vec<Atomic<Node<K, V>>>,
}

impl<K, V> Node<K, V> {
    /// Allocate a new node with the given height (number of levels).
    pub fn new(key: K, value: V, height: usize) -> Node<K, V> {
        assert!(height >= 1);
        let mut next = Vec::with_capacity(height);
        for _ in 0..height {
            next.push(Atomic::null());
        }
        Node {
            key: Some(key),
            value: Some(value),
            next,
        }
    }

    /// Allocate a sentinel head node with a full-height tower.
    pub fn head(height: usize) -> Node<K, V> {
        assert!(height >= 1);
        let mut next = Vec::with_capacity(height);
        for _ in 0..height {
            next.push(Atomic::null());
        }
        Node {
            key: None,
            value: None,
            next,
        }
    }

    /// The number of levels in this node's tower.
    #[inline]
    pub fn height(&self) -> usize {
        self.next.len()
    }

    /// Load the next pointer at `level` with the given ordering.
    #[inline]
    pub fn load_next<'g>(
        &self,
        level: usize,
        ord: Ordering,
        guard: &'g Guard,
    ) -> Shared<'g, Node<K, V>> {
        self.next[level].load(ord, guard)
    }

    /// Return true if the node is logically deleted (level-0 mark bit set).
    #[inline]
    pub fn is_marked(&self) -> bool {
        // Loading with an unprotected guard is safe here because we only read
        // the tag bit, not the pointer payload, and tag bits are written by
        // CAS which is atomic with respect to the load.
        self.next[0]
            .load(Ordering::Relaxed, unsafe { crossbeam_epoch::unprotected() })
            .tag()
            == MARK_TAG
    }

    /// Reference to the key, panicking if this is the head node.
    #[inline]
    pub fn key(&self) -> &K {
        self.key.as_ref().expect("head node has no key")
    }

    /// Reference to the value, panicking if this is the head node.
    #[inline]
    pub fn value(&self) -> &V {
        self.value.as_ref().expect("head node has no value")
    }
}

/// Mark a shared pointer.
#[inline]
pub fn mark_shared<'g, K, V>(ptr: Shared<'g, Node<K, V>>) -> Shared<'g, Node<K, V>> {
    ptr.with_tag(MARK_TAG)
}

/// Return the unmarked version of a shared pointer.
#[inline]
pub fn unmark_shared<'g, K, V>(ptr: Shared<'g, Node<K, V>>) -> Shared<'g, Node<K, V>> {
    ptr.with_tag(0)
}
