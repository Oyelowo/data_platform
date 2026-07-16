//! Sharded LRU block cache for SSTable blocks.
//!
//! The cache is keyed by `(file_number, block_offset)`. Values are `Bytes` so
//! they can be cheaply cloned out of the cache and shared with iterators.
//!
//! The implementation is a safe doubly-linked LRU list per shard. Sharding
//! reduces contention on the hot read path.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use bytes::Bytes;

use crate::FileNumber;

/// Number of shards. Must be a power of two so we can use `& (SHARD_COUNT - 1)`.
const SHARD_COUNT: usize = 16;
const SHARD_MASK: usize = SHARD_COUNT - 1;

/// Key for a cached SSTable block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockCacheKey {
    pub file_number: FileNumber,
    pub offset: u64,
}

/// A sharded LRU cache for fixed-size blocks.
pub struct BlockCache {
    shards: Vec<Mutex<Shard>>,
    max_weight: AtomicUsize,
}

impl BlockCache {
    /// Create a cache with the given total capacity in bytes.
    ///
    /// A capacity of zero is accepted: the cache will always miss and will
    /// never retain inserted blocks.
    pub fn with_capacity(capacity: usize) -> Self {
        let mut shards = Vec::with_capacity(SHARD_COUNT);
        for _ in 0..SHARD_COUNT {
            shards.push(Mutex::new(Shard::new()));
        }
        Self {
            shards,
            max_weight: AtomicUsize::new(capacity),
        }
    }

    /// Return a cached block, or `None` on miss.
    pub fn get(&self, key: BlockCacheKey) -> Option<Bytes> {
        let shard = &self.shards[self.shard_index(&key)];
        let mut shard = shard.lock().unwrap();
        shard.get(key)
    }

    /// Insert a block into the cache. Evicts old entries if the new entry would
    /// exceed the capacity.
    pub fn insert(&self, key: BlockCacheKey, value: Bytes) {
        let weight = value.len();
        let shard = &self.shards[self.shard_index(&key)];
        let mut shard = shard.lock().unwrap();
        shard.insert(key, value, self.max_weight.load(Ordering::Relaxed));
    }

    /// Total capacity in bytes.
    pub fn capacity(&self) -> usize {
        self.max_weight.load(Ordering::Relaxed)
    }

    /// Change the total capacity. Existing entries are not evicted until the
    /// next insert.
    pub fn set_capacity(&self, capacity: usize) {
        self.max_weight.store(capacity, Ordering::Relaxed);
    }

    fn shard_index(&self, key: &BlockCacheKey) -> usize {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        key.hash(&mut hasher);
        (hasher.finish() as usize) & SHARD_MASK
    }
}

struct Shard {
    map: HashMap<BlockCacheKey, Rc<RefCell<Node>>>,
    head: Option<Rc<RefCell<Node>>>,
    tail: Option<Rc<RefCell<Node>>>,
    weight: usize,
}

struct Node {
    key: BlockCacheKey,
    value: Bytes,
    prev: Option<Weak<RefCell<Node>>>,
    next: Option<Rc<RefCell<Node>>>,
}

impl Shard {
    fn new() -> Self {
        Self {
            map: HashMap::new(),
            head: None,
            tail: None,
            weight: 0,
        }
    }

    fn get(&mut self, key: BlockCacheKey) -> Option<Bytes> {
        let node = self.map.get(&key)?.clone();
        let value = node.borrow().value.clone();
        self.move_to_front(&node);
        Some(value)
    }

    fn insert(&mut self, key: BlockCacheKey, value: Bytes, max_weight: usize) {
        let weight = value.len();

        // Update in place if the key already exists.
        if let Some(node) = self.map.get(&key).cloned() {
            {
                let mut n = node.borrow_mut();
                self.weight = self.weight.saturating_sub(n.value.len());
                n.value = value;
            }
            self.weight += weight;
            self.move_to_front(&node);
            return;
        }

        // Evict until we have room for the new block.
        eprintln!("DEBUG insert key={:?} weight={} total_weight={} max_weight={}", key, weight, self.weight, max_weight);
        while self.weight + weight > max_weight && self.tail.is_some() {
            eprintln!("DEBUG evict loop");
            self.evict_lru();
        }

        // If the single block is larger than the cache, do not cache it.
        if weight > max_weight {
            return;
        }

        let node = Rc::new(RefCell::new(Node {
            key,
            value,
            prev: None,
            next: self.head.clone(),
        }));

        if let Some(head) = self.head.as_ref() {
            head.borrow_mut().prev = Some(Rc::downgrade(&node));
        } else {
            // First node: it is both head and tail.
            self.tail = Some(node.clone());
        }

        self.head = Some(node.clone());
        self.map.insert(key, node);
        self.weight += weight;
    }

    fn move_to_front(&mut self, node: &Rc<RefCell<Node>>) {
        // If already at the front, nothing to do.
        if self
            .head
            .as_ref()
            .map(|h| Rc::ptr_eq(h, node))
            .unwrap_or(false)
        {
            return;
        }

        // Detach from current position.
        {
            let n = node.borrow();
            if let Some(prev) = n.prev.as_ref().and_then(|w| w.upgrade()) {
                prev.borrow_mut().next = n.next.clone();
            } else {
                // This node was the head; moving it forward is a no-op.
                return;
            }
            if let Some(next) = n.next.as_ref() {
                next.borrow_mut().prev = n.prev.clone();
            } else {
                // This node was the tail.
                self.tail = n.prev.as_ref().and_then(|w| w.upgrade());
            }
        }

        // Attach to front.
        {
            let mut n = node.borrow_mut();
            n.prev = None;
            n.next = self.head.clone();
        }
        if let Some(head) = self.head.as_ref() {
            head.borrow_mut().prev = Some(Rc::downgrade(node));
        }
        self.head = Some(node.clone());
    }

    fn evict_lru(&mut self) {
        let tail = match self.tail.take() {
            Some(t) => t,
            None => return,
        };

        {
            let n = tail.borrow();
            eprintln!("DEBUG evict key={:?} weight={} total_weight={}", n.key, n.value.len(), self.weight);
            if let Some(prev) = n.prev.as_ref().and_then(|w| w.upgrade()) {
                prev.borrow_mut().next = None;
                self.tail = Some(prev);
            } else {
                // Only node in the list.
                self.head = None;
                self.tail = None;
            }
            self.weight = self.weight.saturating_sub(n.value.len());
            self.map.remove(&n.key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_get_insert() {
        let cache = BlockCache::with_capacity(1024);
        let key = BlockCacheKey {
            file_number: 1,
            offset: 0,
        };
        assert!(cache.get(key).is_none());
        cache.insert(key, Bytes::from_static(b"hello"));
        assert_eq!(cache.get(key).unwrap(), Bytes::from_static(b"hello"));
    }

    #[test]
    fn eviction_by_weight() {
        let cache = BlockCache::with_capacity(10);
        for i in 0..5u8 {
            cache.insert(
                BlockCacheKey {
                    file_number: 1,
                    offset: u64::from(i),
                },
                Bytes::from(vec![i; 3]),
            );
        }

        // The first two entries should have been evicted (3 * 4 = 12 > 10).
        assert!(cache
            .get(BlockCacheKey {
                file_number: 1,
                offset: 0
            })
            .is_none());
        assert!(cache
            .get(BlockCacheKey {
                file_number: 1,
                offset: 1
            })
            .is_none());

        // Most-recently used entries should still be present.
        for i in 2..5u8 {
            assert_eq!(
                cache
                    .get(BlockCacheKey {
                        file_number: 1,
                        offset: u64::from(i)
                    })
                    .unwrap(),
                Bytes::from(vec![i; 3])
            );
        }
    }

    #[test]
    fn access_moves_to_front() {
        let cache = BlockCache::with_capacity(10);
        for i in 0..4u8 {
            cache.insert(
                BlockCacheKey {
                    file_number: 1,
                    offset: u64::from(i),
                },
                Bytes::from(vec![i; 3]),
            );
        }

        // Touch offset 0 so it becomes most recently used.
        assert_eq!(
            cache
                .get(BlockCacheKey {
                    file_number: 1,
                    offset: 0
                })
                .unwrap(),
            Bytes::from(vec![0u8; 3])
        );

        // Insert one more block; this should evict offset 1, not offset 0.
        cache.insert(
            BlockCacheKey {
                file_number: 1,
                offset: 4,
            },
            Bytes::from(vec![4u8; 3]),
        );

        assert!(cache
            .get(BlockCacheKey {
                file_number: 1,
                offset: 1
            })
            .is_none());
        assert!(cache
            .get(BlockCacheKey {
                file_number: 1,
                offset: 0
            })
            .is_some());
    }

    #[test]
    fn oversized_block_not_cached() {
        let cache = BlockCache::with_capacity(10);
        cache.insert(
            BlockCacheKey {
                file_number: 1,
                offset: 0,
            },
            Bytes::from(vec![0u8; 100]),
        );
        assert!(cache
            .get(BlockCacheKey {
                file_number: 1,
                offset: 0
            })
            .is_none());
    }

    #[test]
    fn zero_capacity_cache_never_caches() {
        let cache = BlockCache::with_capacity(0);
        cache.insert(
            BlockCacheKey {
                file_number: 1,
                offset: 0,
            },
            Bytes::from_static(b"x"),
        );
        assert!(cache
            .get(BlockCacheKey {
                file_number: 1,
                offset: 0
            })
            .is_none());
    }

    #[test]
    fn update_existing_key() {
        let cache = BlockCache::with_capacity(1024);
        let key = BlockCacheKey {
            file_number: 1,
            offset: 0,
        };
        cache.insert(key, Bytes::from_static(b"old"));
        cache.insert(key, Bytes::from_static(b"new"));
        assert_eq!(cache.get(key).unwrap(), Bytes::from_static(b"new"));
    }
}
