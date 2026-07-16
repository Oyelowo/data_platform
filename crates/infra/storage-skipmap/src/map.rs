//! Lock-free skip-map implementation.

use crossbeam_epoch::{Atomic, Guard, Owned, Shared};
use rand::RngCore;
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering as MemOrdering};
use std::sync::Mutex;

use crate::node::{mark_shared, unmark_shared, Node, MARK_TAG};

/// Maximum height of any node tower.
const MAX_HEIGHT: usize = 32;

/// Probability of advancing to the next level (1 / P_ADVANCE).
const P_ADVANCE: u32 = 2;

/// Result of a search operation.
struct Position<'g, K, V> {
    /// Predecessor node at each level.
    preds: Vec<Shared<'g, Node<K, V>>>,
    /// Successor shared pointer at each level.
    succs: Vec<Shared<'g, Node<K, V>>>,
}

/// A lock-free ordered map backed by a skip list.
pub struct SkipMap<K, V> {
    /// Sentinel head node with a full-height tower.
    head: Atomic<Node<K, V>>,
    /// Highest level currently in use (hint, never decreases).
    max_height: AtomicUsize,
    /// Approximate number of entries.
    len: AtomicUsize,
    /// Random number generator for height selection.
    rng: Mutex<rand::rngs::StdRng>,
}

impl<K, V> fmt::Debug for SkipMap<K, V>
where
    K: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SkipMap").field("len", &self.len()).finish()
    }
}

impl<K, V> SkipMap<K, V> {
    /// Approximate number of entries.
    pub fn len(&self) -> usize {
        self.len.load(MemOrdering::Relaxed)
    }

    /// True if the map contains no entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<K, V> SkipMap<K, V>
where
    K: Ord + Send + Sync + 'static,
    V: Send + Sync + 'static,
{
    /// Create an empty skip-map.
    pub fn new() -> Self {
        Self {
            head: Atomic::new(Node::head(MAX_HEIGHT)),
            max_height: AtomicUsize::new(1),
            len: AtomicUsize::new(0),
            rng: Mutex::new(rand::SeedableRng::from_entropy()),
        }
    }

    /// Look up a key and clone its value.
    pub fn get(&self, key: &K) -> Option<V>
    where
        V: Clone,
    {
        let guard = crossbeam_epoch::pin();
        let found = self.search_found(key, &guard);
        found.map(|n| unsafe { n.deref() }.value().clone())
    }

    /// Insert a key-value pair. Returns the previous value if the key existed.
    pub fn insert(&self, key: K, value: V) -> Option<V>
    where
        V: Clone,
        K: Clone,
    {
        let guard = crossbeam_epoch::pin();
        let height = self.random_height();
        let new: Shared<'_, Node<K, V>> =
            Owned::new(Node::new(key.clone(), value, height)).into_shared(&guard);

        loop {
            let pos = self.search(&key, &guard);

            if let Some(n) = self.found_node(&pos, &key) {
                let node = unsafe { n.deref() };
                if node.is_marked() {
                    continue;
                }

                // Replace an existing entry by logically deleting the old node
                // and inserting a new one. In-place mutation would race with
                // concurrent readers, so we never mutate values after
                // publication.
                let old = node.value().clone();

                // Link level 0 of the new node to the successor of the old
                // node so that it is visible immediately after the CAS.
                let next0 = node.next[0].load(MemOrdering::Acquire, &guard);
                unsafe {
                    new.deref().next[0].store(unmark_shared(next0), MemOrdering::Relaxed);
                }

                // Attempt to splice the new node in front of the old one at
                // level 0. If this fails, another thread changed the list and
                // we retry the whole operation.
                let pred0 = pos.preds[0];
                if unsafe { pred0.deref() }
                    .next[0]
                    .compare_exchange(n, new, MemOrdering::SeqCst, MemOrdering::Relaxed, &guard)
                    .is_err()
                {
                    continue;
                }

                // Mark the old node as deleted. From this point on find()
                // will skip it because its level-0 next pointer is tagged.
                let _ = node.next[0].compare_exchange(
                    unmark_shared(next0),
                    mark_shared(next0),
                    MemOrdering::SeqCst,
                    MemOrdering::Relaxed,
                    &guard,
                );

                // Physically unlink the old node from all levels before
                // retiring it, so no future reader can dereference it after
                // reclamation.
                self.unlink_all(&pos, n, &guard);

                // Build the tower for the new node and retire the old one.
                self.build_tower(new, &pos, &guard);
                unsafe {
                    guard.defer_destroy(n);
                }
                return Some(old);
            }

            // Link level 0.
            unsafe {
                new.deref().next[0].store(pos.succs[0], MemOrdering::Relaxed);
            }

            if unsafe { pos.preds[0].deref() }
                .next[0]
                .compare_exchange(
                    pos.succs[0],
                    new,
                    MemOrdering::SeqCst,
                    MemOrdering::Relaxed,
                    &guard,
                )
                .is_ok()
            {
                self.len.fetch_add(1, MemOrdering::Relaxed);
                self.build_tower(new, &pos, &guard);
                return None;
            }

            // CAS failed; retry.
        }
    }

    /// Remove a key and return its value.
    pub fn remove(&self, key: &K) -> Option<V>
    where
        V: Clone,
    {
        let guard = crossbeam_epoch::pin();

        loop {
            let pos = self.search(key, &guard);
            let n = self.found_node(&pos, key)?;
            let node = unsafe { n.deref() };

            if node.is_marked() {
                return None;
            }

            let old = node.value().clone();

            // Logical delete: mark level 0.
            let next0 = node.next[0].load(MemOrdering::SeqCst, &guard);
            let marked = mark_shared(next0);
            if node.next[0]
                .compare_exchange(
                    next0,
                    marked,
                    MemOrdering::SeqCst,
                    MemOrdering::Relaxed,
                    &guard,
                )
                .is_err()
            {
                continue;
            }

            self.len.fetch_sub(1, MemOrdering::Relaxed);

            // Physical delete at all levels.
            self.unlink_all(&pos, n, &guard);

            // Retire the node.
            unsafe {
                guard.defer_destroy(n);
            }

            return Some(old);
        }
    }

    /// Return true if the map contains the key.
    pub fn contains_key(&self, key: &K) -> bool {
        let guard = crossbeam_epoch::pin();
        self.search_found(key, &guard).is_some()
    }

    /// Return all entries in `[start, end)` as a sorted snapshot.
    pub fn range(&self, start: Option<&K>, end: Option<&K>) -> Vec<(K, V)>
    where
        K: Clone,
        V: Clone,
    {
        let guard = crossbeam_epoch::pin();
        let mut out = Vec::new();
        let mut curr = self.level0_head(&guard);

        while let Some(node) = unsafe { curr.as_ref() } {
            let next = node.next[0].load(MemOrdering::Acquire, &guard);

            // Skip logically deleted nodes. Their level-0 next pointer carries
            // the mark bit.
            if node.is_marked() {
                curr = next;
                continue;
            }

            let key = node.key();

            if let Some(s) = start && key < s {
                curr = next;
                continue;
            }
            if let Some(e) = end && key >= e {
                break;
            }

            out.push((key.clone(), node.value().clone()));
            curr = next;
        }

        out
    }

    /// Return all entries as a sorted snapshot.
    pub fn iter(&self) -> Vec<(K, V)>
    where
        K: Clone,
        V: Clone,
    {
        self.range(None, None)
    }
}

impl<K, V> SkipMap<K, V>
where
    K: Ord,
{
    fn level0_head<'g>(&self, guard: &'g Guard) -> Shared<'g, Node<K, V>> {
        let head = self.head.load(MemOrdering::Acquire, guard);
        let head_node = unsafe { head.deref() };
        head_node.next[0].load(MemOrdering::Acquire, guard)
    }

    /// Search for `key`. Returns predecessors and successors at every level.
    fn search<'g>(&self, key: &K, guard: &'g Guard) -> Position<'g, K, V> {
        let mut preds = Vec::with_capacity(MAX_HEIGHT);
        let mut succs = Vec::with_capacity(MAX_HEIGHT);

        let head = self.head.load(MemOrdering::Acquire, guard);

        let mut max_level = self.max_height.load(MemOrdering::Relaxed);
        if max_level > MAX_HEIGHT {
            max_level = MAX_HEIGHT;
        }

        'retry: loop {
            preds.clear();
            succs.clear();
            preds.resize(MAX_HEIGHT, head);
            succs.resize(MAX_HEIGHT, Shared::null());

            let mut level = max_level;
            let mut pred = head;

            while level > 0 {
                level -= 1;
                let pred_node = unsafe { pred.deref() };
                let mut curr = pred_node.next[level].load(MemOrdering::Acquire, guard);

                loop {
                    if curr.tag() == MARK_TAG {
                        // Predecessor pointer is marked; restart.
                        continue 'retry;
                    }

                    match unsafe { curr.as_ref() } {
                        Some(node) => {
                            let next = node.next[level].load(MemOrdering::Acquire, guard);

                            // A node whose level-0 next pointer is marked is
                            // logically deleted. At level 0 the mark is stored
                            // directly in `next`; at upper levels we detect it
                            // via `is_marked` and help complete the physical
                            // deletion so searches cannot get trapped behind a
                            // deleted predecessor.
                            let marked = next.tag() == MARK_TAG
                                || (level > 0 && node.is_marked());
                            if marked {
                                let unmarked_next = unmark_shared(next);
                                let _ = pred_node.next[level].compare_exchange(
                                    curr,
                                    unmarked_next,
                                    MemOrdering::SeqCst,
                                    MemOrdering::Relaxed,
                                    guard,
                                );
                                curr = unmarked_next;
                                continue;
                            }

                            if node.key() < key {
                                pred = curr;
                                curr = next;
                                continue;
                            }

                            preds[level] = pred;
                            succs[level] = curr;
                            break;
                        }
                        None => {
                            preds[level] = pred;
                            succs[level] = Shared::null();
                            break;
                        }
                    }
                }
            }

            return Position { preds, succs };
        }
    }

    /// Return the found node from a search position, if it matches the key and
    /// is not marked.
    fn found_node<'g>(
        &self,
        pos: &Position<'g, K, V>,
        key: &K,
    ) -> Option<Shared<'g, Node<K, V>>> {
        unsafe { pos.succs[0].as_ref() }.and_then(|node| {
            if node.key() == key && !node.is_marked() {
                Some(pos.succs[0])
            } else {
                None
            }
        })
    }

    /// Convenience: search and return the matching unmarked node.
    fn search_found<'g>(&self, key: &K, guard: &'g Guard) -> Option<Shared<'g, Node<K, V>>> {
        let pos = self.search(key, guard);
        self.found_node(&pos, key)
    }

    /// Build the upper levels of a newly inserted node.
    fn build_tower(&self, node: Shared<Node<K, V>>, pos: &Position<K, V>, guard: &Guard) {
        let height = unsafe { node.deref() }.height();
        let mut pos = Position {
            preds: pos.preds.clone(),
            succs: pos.succs.clone(),
        };

        for level in 1..height {
            loop {
                let pred = pos.preds[level];
                let pred_node = unsafe { pred.deref() };
                let succ = pos.succs[level];
                unsafe {
                    node.deref().next[level].store(succ, MemOrdering::Relaxed);
                }

                if pred_node
                    .next[level]
                    .compare_exchange(succ, node, MemOrdering::SeqCst, MemOrdering::Relaxed, guard)
                    .is_ok()
                {
                    break;
                }

                // Re-search and retry. If the node was removed, stop building.
                let key = unsafe { node.deref() }.key();
                pos = self.search(key, guard);
                if self.found_node(&pos, key) != Some(node) {
                    return;
                }
                // Keep retrying at this level with the refreshed position.
            }
        }

        // Update max height hint.
        let mut max = self.max_height.load(MemOrdering::Relaxed);
        while height > max {
            match self.max_height.compare_exchange_weak(
                max,
                height,
                MemOrdering::Relaxed,
                MemOrdering::Relaxed,
            ) {
                Ok(_) => break,
                Err(m) => max = m,
            }
        }
    }

    /// Physically unlink a marked node at all levels using the search position.
    fn unlink_all(&self, pos: &Position<K, V>, node: Shared<Node<K, V>>, guard: &Guard) {
        let height = unsafe { node.deref() }.height();
        for level in 0..height {
            let pred = pos.preds[level];
            let pred_node = unsafe { pred.deref() };
            let next = unsafe { node.deref() }.next[level].load(MemOrdering::SeqCst, guard);
            let unmarked = unmark_shared(next);
            let _ = pred_node.next[level].compare_exchange(
                node,
                unmarked,
                MemOrdering::SeqCst,
                MemOrdering::Relaxed,
                guard,
            );
        }
    }

    /// Generate a random tower height.
    fn random_height(&self) -> usize {
        let mut rng = self.rng.lock().expect("rng mutex poisoned");
        let mut height = 1;
        while height < MAX_HEIGHT && rng.next_u32().is_multiple_of(P_ADVANCE) {
            height += 1;
        }
        height
    }
}

impl<K, V> Default for SkipMap<K, V>
where
    K: Ord + Send + Sync + 'static,
    V: Send + Sync + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}
