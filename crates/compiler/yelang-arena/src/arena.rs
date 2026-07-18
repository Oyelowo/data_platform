use slotmap::{Key, SecondaryMap, SlotMap as InnerSlotMap, SparseSecondaryMap};

pub use slotmap::Key as SlotMapKey;
/// Re-export the slotmap key trait and macro so callers can define their own
/// typed keys (e.g. `ExprId`, `PatId`) and use them with the arenas below.
pub use slotmap::new_key_type;

/// A generational arena allocator with typed keys.
///
/// Wrapper around `slotmap::SlotMap<K, V>`. Each key carries a generation, so
/// stale IDs cannot accidentally access data that was removed and reused.
/// This is the right tool for HIR expression/pattern/type nodes, incremental
/// structures, and any collection where IDs must stay valid across removals.
#[derive(Debug, Clone)]
pub struct Arena<K: Key, T> {
    inner: InnerSlotMap<K, T>,
}

impl<K: Key, T> Arena<K, T> {
    pub fn new() -> Self {
        Self {
            inner: InnerSlotMap::with_key(),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: InnerSlotMap::with_capacity_and_key(capacity),
        }
    }

    pub fn insert(&mut self, value: T) -> K {
        self.inner.insert(value)
    }

    pub fn get(&self, key: K) -> Option<&T> {
        self.inner.get(key)
    }

    pub fn get_mut(&mut self, key: K) -> Option<&mut T> {
        self.inner.get_mut(key)
    }

    pub fn remove(&mut self, key: K) -> Option<T> {
        self.inner.remove(key)
    }

    pub fn contains_key(&self, key: K) -> bool {
        self.inner.contains_key(key)
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (K, &T)> {
        self.inner.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (K, &mut T)> {
        self.inner.iter_mut()
    }

    pub fn keys(&self) -> impl Iterator<Item = K> + '_ {
        self.inner.keys()
    }

    pub fn values(&self) -> impl Iterator<Item = &T> {
        self.inner.values()
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.inner.values_mut()
    }

    pub fn reserve(&mut self, additional: usize) {
        self.inner.reserve(additional);
    }

    pub fn clear(&mut self) {
        self.inner.clear();
    }
}

impl<K: Key, T> Default for Arena<K, T> {
    fn default() -> Self {
        Self::new()
    }
}

// ----------------------------------------------------------------------------
// SecondaryMap
// ----------------------------------------------------------------------------

/// A dense map from an arena key to another value type.
///
/// Must have the same key domain as the `Arena` it indexes into.
#[derive(Debug, Clone)]
pub struct ArenaMap<K: Key, T> {
    inner: SecondaryMap<K, T>,
}

impl<K: Key, T> ArenaMap<K, T> {
    pub fn new() -> Self {
        Self {
            inner: SecondaryMap::new(),
        }
    }

    pub fn insert(&mut self, key: K, value: T) -> Option<T> {
        self.inner.insert(key, value)
    }

    pub fn get(&self, key: K) -> Option<&T> {
        self.inner.get(key)
    }

    pub fn get_mut(&mut self, key: K) -> Option<&mut T> {
        self.inner.get_mut(key)
    }

    pub fn contains_key(&self, key: K) -> bool {
        self.inner.contains_key(key)
    }

    pub fn remove(&mut self, key: K) -> Option<T> {
        self.inner.remove(key)
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (K, &T)> {
        self.inner.iter()
    }

    pub fn clear(&mut self) {
        self.inner.clear();
    }
}

impl<K: Key, T> Default for ArenaMap<K, T> {
    fn default() -> Self {
        Self::new()
    }
}

// ----------------------------------------------------------------------------
// SparseSecondaryMap
// ----------------------------------------------------------------------------

/// A sparse map from an arena key to another value type.
/// More memory-efficient than `ArenaMap` when few keys are populated.
#[derive(Debug, Clone)]
pub struct SparseArenaMap<K: Key, T> {
    inner: SparseSecondaryMap<K, T>,
}

impl<K: Key, T> SparseArenaMap<K, T> {
    pub fn new() -> Self {
        Self {
            inner: SparseSecondaryMap::new(),
        }
    }

    pub fn insert(&mut self, key: K, value: T) -> Option<T> {
        self.inner.insert(key, value)
    }

    pub fn get(&self, key: K) -> Option<&T> {
        self.inner.get(key)
    }

    pub fn get_mut(&mut self, key: K) -> Option<&mut T> {
        self.inner.get_mut(key)
    }

    pub fn contains_key(&self, key: K) -> bool {
        self.inner.contains_key(key)
    }

    pub fn remove(&mut self, key: K) -> Option<T> {
        self.inner.remove(key)
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn clear(&mut self) {
        self.inner.clear();
    }
}

impl<K: Key, T> Default for SparseArenaMap<K, T> {
    fn default() -> Self {
        Self::new()
    }
}

// ----------------------------------------------------------------------------
// Untyped default-key arena (kept for callers that do not need typed keys)
// ----------------------------------------------------------------------------

new_key_type! {
    /// An opaque, untyped key into the default `Arena`.
    pub struct ArenaKey;
}

/// Alias for the common case of an arena that does not need domain-specific keys.
pub type DefaultArena<T> = Arena<ArenaKey, T>;

#[cfg(test)]
mod tests {
    use super::*;

    new_key_type! {
        struct TestKey;
    }

    #[test]
    fn arena_basic() {
        let mut arena = Arena::<TestKey, _>::new();
        let k1 = arena.insert("hello");
        let k2 = arena.insert("world");

        assert_eq!(arena.get(k1), Some(&"hello"));
        assert_eq!(arena.get(k2), Some(&"world"));
        assert_eq!(arena.len(), 2);
    }

    #[test]
    fn arena_map() {
        let mut arena = Arena::<TestKey, _>::new();
        let k1 = arena.insert(100);
        let k2 = arena.insert(200);

        let mut map = ArenaMap::<TestKey, _>::new();
        map.insert(k1, "a");
        map.insert(k2, "b");

        assert_eq!(map.get(k1), Some(&"a"));
        assert_eq!(map.get(k2), Some(&"b"));
    }

    #[test]
    fn arena_remove() {
        let mut arena = Arena::<TestKey, _>::new();
        let k = arena.insert(42);
        assert!(arena.contains_key(k));
        assert_eq!(arena.remove(k), Some(42));
        assert!(!arena.contains_key(k));
    }
}
