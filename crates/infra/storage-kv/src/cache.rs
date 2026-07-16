//! Sharded LRU block cache for SSTable blocks.
//!
//! The cache is keyed by `(file_number, block_offset)`. Values are `Bytes` so
//! they can be cheaply cloned out of the cache and shared with iterators.
//!
//! The implementation is a safe doubly-linked LRU list per shard. Sharding
//! reduces contention on the hot read path.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};

use bytes::Bytes;

use crate::FileNumber;
use crate::metrics::Metrics;

/// Number of shards. Must be a power of two so we can use `& (SHARD_COUNT - 1)`.
const SHARD_COUNT: usize = 16;
const SHARD_MASK: usize = SHARD_COUNT - 1;

/// Key for a cached SSTable block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockCacheKey {
    pub file_number: FileNumber,
    pub offset: u64,
}

/// The two block-cache tiers shared by all SSTable readers.
///
/// * The **hot tier** holds decompressed blocks; serving a hit costs nothing
///   but a lookup.
/// * The optional **cold tier** holds blocks exactly as stored on disk
///   (compressed payload plus trailer); a hit still needs decompression but
///   avoids disk I/O.  The cold tier is usually disabled because the OS page
///   cache already caches the compressed file contents — it exists for
///   direct-I/O deployments where the page cache is bypassed.
///
/// Both tiers use the same sharded-LRU [`BlockCache`] implementation; only
/// the stored values differ.
#[derive(Clone)]
pub struct BlockCaches {
    hot: Arc<BlockCache>,
    cold: Option<Arc<BlockCache>>,
    metrics: Arc<Metrics>,
}

impl BlockCaches {
    /// Create a tier pair.  `cold` of `None` disables the cold tier.
    pub fn new(hot: Arc<BlockCache>, cold: Option<Arc<BlockCache>>, metrics: Arc<Metrics>) -> Self {
        Self { hot, cold, metrics }
    }

    /// Hot tier (decompressed blocks).
    pub fn hot(&self) -> &BlockCache {
        &self.hot
    }

    /// Cold tier (stored bytes), if enabled.
    pub fn cold(&self) -> Option<&BlockCache> {
        self.cold.as_deref()
    }

    /// Metrics shared by all readers using these caches.
    pub fn metrics(&self) -> &Metrics {
        &self.metrics
    }
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
        let total = self.max_weight.load(Ordering::Relaxed);
        let per_shard = total / SHARD_COUNT;
        let shard = &self.shards[self.shard_index(&key)];
        let mut shard = shard.lock().unwrap();
        shard.insert(key, value, per_shard);
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

    /// Total weight currently cached across all shards. Exposed for tests.
    #[cfg(test)]
    pub fn total_weight(&self) -> usize {
        self.shards.iter().map(|s| s.lock().unwrap().weight).sum()
    }

    fn shard_index(&self, key: &BlockCacheKey) -> usize {
        shard_index(key)
    }
}

fn shard_index(key: &BlockCacheKey) -> usize {
    // Deterministic splitmix64-style hash. We avoid DefaultHasher because
    // its behaviour can vary between HashMap instances, and we need the
    // same key to land in the same shard for every call.
    let mut x = key
        .file_number
        .wrapping_mul(0x9e37_79b9_7f4a_7c15)
        .wrapping_add(key.offset);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x = x ^ (x >> 31);
    (x as usize) & SHARD_MASK
}

struct Shard {
    map: HashMap<BlockCacheKey, Arc<Mutex<Node>>>,
    head: Option<Arc<Mutex<Node>>>,
    tail: Option<Arc<Mutex<Node>>>,
    weight: usize,
}

struct Node {
    key: BlockCacheKey,
    value: Bytes,
    prev: Option<Weak<Mutex<Node>>>,
    next: Option<Arc<Mutex<Node>>>,
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
        let value = node.lock().unwrap().value.clone();
        self.move_to_front(&node);
        Some(value)
    }

    fn insert(&mut self, key: BlockCacheKey, value: Bytes, max_weight: usize) {
        let weight = value.len();

        // Update in place if the key already exists.
        if let Some(node) = self.map.get(&key).cloned() {
            {
                let mut n = node.lock().unwrap();
                self.weight = self.weight.saturating_sub(n.value.len());
                n.value = value;
            }
            self.weight += weight;
            self.move_to_front(&node);
            return;
        }

        // Evict until we have room for the new block.
        while self.weight + weight > max_weight && self.tail.is_some() {
            self.evict_lru();
        }

        // If the single block is larger than the cache, do not cache it.
        if weight > max_weight {
            return;
        }

        let node = Arc::new(Mutex::new(Node {
            key,
            value,
            prev: None,
            next: self.head.clone(),
        }));

        if let Some(head) = self.head.as_ref() {
            head.lock().unwrap().prev = Some(Arc::downgrade(&node));
        } else {
            // First node: it is both head and tail.
            self.tail = Some(node.clone());
        }

        self.head = Some(node.clone());
        self.map.insert(key, node);
        self.weight += weight;
    }

    fn move_to_front(&mut self, node: &Arc<Mutex<Node>>) {
        // If already at the front, nothing to do.
        if self
            .head
            .as_ref()
            .map(|h| Arc::ptr_eq(h, node))
            .unwrap_or(false)
        {
            return;
        }

        // Detach from current position.
        {
            let n = node.lock().unwrap();
            if let Some(prev) = n.prev.as_ref().and_then(|w| w.upgrade()) {
                prev.lock().unwrap().next = n.next.clone();
            } else {
                // This node was the head; moving it forward is a no-op.
                return;
            }
            if let Some(next) = n.next.as_ref() {
                next.lock().unwrap().prev = n.prev.clone();
            } else {
                // This node was the tail.
                self.tail = n.prev.as_ref().and_then(|w| w.upgrade());
            }
        }

        // Attach to front.
        {
            let mut n = node.lock().unwrap();
            n.prev = None;
            n.next = self.head.clone();
        }
        if let Some(head) = self.head.as_ref() {
            head.lock().unwrap().prev = Some(Arc::downgrade(node));
        }
        self.head = Some(node.clone());
    }

    fn evict_lru(&mut self) {
        let tail = match self.tail.take() {
            Some(t) => t,
            None => return,
        };

        {
            let n = tail.lock().unwrap();
            if let Some(prev) = n.prev.as_ref().and_then(|w| w.upgrade()) {
                prev.lock().unwrap().next = None;
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

    /// Return `count` distinct keys that all hash to `target_shard`.
    fn keys_in_shard(target_shard: usize, count: usize) -> Vec<BlockCacheKey> {
        let mut keys = Vec::with_capacity(count);
        for offset in 0..10_000u64 {
            let key = BlockCacheKey {
                file_number: 1,
                offset,
            };
            if shard_index(&key) == target_shard {
                keys.push(key);
                if keys.len() == count {
                    return keys;
                }
            }
        }
        panic!("could not find {} keys for shard {}", count, target_shard);
    }

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
        // Target a single shard so the eviction order is deterministic.
        // Capacity is 3 * block_size * shard_count so each shard can hold
        // exactly three 3-byte blocks.
        let cache = BlockCache::with_capacity(3 * 3 * SHARD_COUNT);
        let keys = keys_in_shard(0, 5);
        for (i, key) in keys.iter().enumerate() {
            cache.insert(*key, Bytes::from(vec![i as u8; 3]));
        }

        // The first two entries should have been evicted.
        assert!(cache.get(keys[0]).is_none());
        assert!(cache.get(keys[1]).is_none());

        // Most-recently used entries should still be present.
        for (i, key) in keys.iter().enumerate().skip(2) {
            assert_eq!(cache.get(*key).unwrap(), Bytes::from(vec![i as u8; 3]));
        }
    }

    #[test]
    fn access_moves_to_front() {
        // Capacity is 4 * block_size * shard_count: each shard holds four
        // 3-byte blocks. We use keys that all hit shard 0.
        let cache = BlockCache::with_capacity(4 * 3 * SHARD_COUNT);
        let keys = keys_in_shard(0, 4);
        for (i, key) in keys.iter().enumerate() {
            cache.insert(*key, Bytes::from(vec![i as u8; 3]));
        }

        // Touch the first key so it becomes most recently used.
        assert_eq!(cache.get(keys[0]).unwrap(), Bytes::from(vec![0u8; 3]));

        // Insert one more block into the same shard; this should evict the
        // second key, not the recently touched first key.
        let extra = keys_in_shard(0, 5)[4];
        cache.insert(extra, Bytes::from(vec![4u8; 3]));

        assert!(cache.get(keys[1]).is_none());
        assert!(cache.get(keys[0]).is_some());
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
        assert!(
            cache
                .get(BlockCacheKey {
                    file_number: 1,
                    offset: 0
                })
                .is_none()
        );
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
        assert!(
            cache
                .get(BlockCacheKey {
                    file_number: 1,
                    offset: 0
                })
                .is_none()
        );
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
