use slotmap::{DefaultKey, SlotMap as InnerSlotMap, SecondaryMap, SparseSecondaryMap};

/// An arena allocator that assigns stable keys to values.
///
/// Wrapper around `slotmap::SlotMap`.
/// Used for interning HIR nodes, allocating bodies, and any case where
/// we need O(1) lookup by a stable handle.
///
/// The key type is `ArenaKey` (a newtype around `DefaultKey`).
#[derive(Debug, Clone)]
pub struct Arena<T> {
    inner: InnerSlotMap<DefaultKey, T>,
}

/// A stable key into an `Arena`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ArenaKey(DefaultKey);

impl<T> Arena<T> {
    pub fn new() -> Self {
        Self {
            inner: InnerSlotMap::new(),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: InnerSlotMap::with_capacity(capacity),
        }
    }

    pub fn insert(&mut self, value: T) -> ArenaKey {
        ArenaKey(self.inner.insert(value))
    }

    pub fn get(&self, key: ArenaKey) -> Option<&T> {
        self.inner.get(key.0)
    }

    pub fn get_mut(&mut self, key: ArenaKey) -> Option<&mut T> {
        self.inner.get_mut(key.0)
    }

    pub fn remove(&mut self, key: ArenaKey) -> Option<T> {
        self.inner.remove(key.0)
    }

    pub fn contains_key(&self, key: ArenaKey) -> bool {
        self.inner.contains_key(key.0)
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (ArenaKey, &T)> {
        self.inner.iter().map(|(k, v)| (ArenaKey(k), v))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (ArenaKey, &mut T)> {
        self.inner.iter_mut().map(|(k, v)| (ArenaKey(k), v))
    }

    pub fn keys(&self) -> impl Iterator<Item = ArenaKey> + '_ {
        self.inner.keys().map(ArenaKey)
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

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Self::new()
    }
}

// Note: IntoIterator is not implemented because slotmap does not export
// its iterator types publicly. Use `iter()` and `iter_mut()` instead.

// ----------------------------------------------------------------------------
// SecondaryMap
// ----------------------------------------------------------------------------

/// A dense map from `ArenaKey` to another value type.
///
/// Must have the same key domain as the `Arena` it indexes into.
#[derive(Debug, Clone)]
pub struct ArenaMap<T> {
    inner: SecondaryMap<DefaultKey, T>,
}

impl<T> ArenaMap<T> {
    pub fn new() -> Self {
        Self {
            inner: SecondaryMap::new(),
        }
    }

    pub fn insert(&mut self, key: ArenaKey, value: T) -> Option<T> {
        self.inner.insert(key.0, value)
    }

    pub fn get(&self, key: ArenaKey) -> Option<&T> {
        self.inner.get(key.0)
    }

    pub fn get_mut(&mut self, key: ArenaKey) -> Option<&mut T> {
        self.inner.get_mut(key.0)
    }

    pub fn contains_key(&self, key: ArenaKey) -> bool {
        self.inner.contains_key(key.0)
    }

    pub fn remove(&mut self, key: ArenaKey) -> Option<T> {
        self.inner.remove(key.0)
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (ArenaKey, &T)> {
        self.inner.iter().map(|(k, v)| (ArenaKey(k), v))
    }

    pub fn clear(&mut self) {
        self.inner.clear();
    }
}

impl<T> Default for ArenaMap<T> {
    fn default() -> Self {
        Self::new()
    }
}

// ----------------------------------------------------------------------------
// SparseSecondaryMap
// ----------------------------------------------------------------------------

/// A sparse map from `ArenaKey` to another value type.
/// More memory-efficient than `ArenaMap` when few keys are populated.
#[derive(Debug, Clone)]
pub struct SparseArenaMap<T> {
    inner: SparseSecondaryMap<DefaultKey, T>,
}

impl<T> SparseArenaMap<T> {
    pub fn new() -> Self {
        Self {
            inner: SparseSecondaryMap::new(),
        }
    }

    pub fn insert(&mut self, key: ArenaKey, value: T) -> Option<T> {
        self.inner.insert(key.0, value)
    }

    pub fn get(&self, key: ArenaKey) -> Option<&T> {
        self.inner.get(key.0)
    }

    pub fn get_mut(&mut self, key: ArenaKey) -> Option<&mut T> {
        self.inner.get_mut(key.0)
    }

    pub fn contains_key(&self, key: ArenaKey) -> bool {
        self.inner.contains_key(key.0)
    }

    pub fn remove(&mut self, key: ArenaKey) -> Option<T> {
        self.inner.remove(key.0)
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

impl<T> Default for SparseArenaMap<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arena_basic() {
        let mut arena = Arena::new();
        let k1 = arena.insert("hello");
        let k2 = arena.insert("world");

        assert_eq!(arena.get(k1), Some(&"hello"));
        assert_eq!(arena.get(k2), Some(&"world"));
        assert_eq!(arena.len(), 2);
    }

    #[test]
    fn arena_map() {
        let mut arena = Arena::new();
        let k1 = arena.insert(100);
        let k2 = arena.insert(200);

        let mut map = ArenaMap::new();
        map.insert(k1, "a");
        map.insert(k2, "b");

        assert_eq!(map.get(k1), Some(&"a"));
        assert_eq!(map.get(k2), Some(&"b"));
    }

    #[test]
    fn arena_remove() {
        let mut arena = Arena::new();
        let k = arena.insert(42);
        assert!(arena.contains_key(k));
        assert_eq!(arena.remove(k), Some(42));
        assert!(!arena.contains_key(k));
    }
}
