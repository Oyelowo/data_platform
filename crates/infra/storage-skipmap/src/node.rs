//! Node layout and low-level helpers for the lock-free skip-map.

use crossbeam_epoch::{Atomic, Guard, Shared};
use parking_lot::{MappedMutexGuard, Mutex, MutexGuard};
use std::sync::atomic::{AtomicBool, Ordering};

/// Tag bit used to mark a node's level-0 next pointer as logically deleted.
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
    ///
    /// A per-node mutex protects value mutations. The tower of next pointers
    /// remains lock-free, so structural updates (insert/remove) proceed without
    /// blocking on sibling nodes.
    pub value: Option<Mutex<V>>,
    /// Tower of next pointers. Index 0 is the ground-level linked list.
    pub next: Vec<Atomic<Node<K, V>>>,
    /// Set to `true` once the node has been logically removed. Only the thread
    /// that flips this flag from `false` to `true` may mark, unlink, and retire
    /// the node, preventing double-retire races between replace and remove.
    pub removed: AtomicBool,
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
            value: Some(Mutex::new(value)),
            next,
            removed: AtomicBool::new(false),
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
            removed: AtomicBool::new(false),
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

    /// Lock and return a reference to the value, panicking if this is the head
    /// node.
    #[inline]
    pub fn value(&self) -> MappedMutexGuard<'_, V> {
        MutexGuard::map(
            self.value.as_ref().expect("head node has no value").lock(),
            |v| v,
        )
    }

    /// Atomically swap the value and return the previous value.
    #[inline]
    pub fn swap_value(&self, new_value: V) -> V {
        let mut guard = self.value.as_ref().expect("head node has no value").lock();
        std::mem::replace(&mut *guard, new_value)
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
