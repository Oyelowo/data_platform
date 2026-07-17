//! Dense, typed-index vector.
//!
//! `IndexVec<K, V>` is a `Vec<V>` indexed by a typed key `K` (e.g. `DefId`).
//! It provides the cache locality of a contiguous array while preventing
//! accidental mixing of different ID spaces at the type level.
//!
//! Keys are 1-based externally but 0-based in the underlying `Vec`.

use std::fmt;
use std::marker::PhantomData;
use std::ops::{Index, IndexMut};

/// A typed index into an [`IndexVec`].
///
/// Implementations are provided for the `Id<T>` newtypes in `crate::id`.
pub trait Idx: Copy + Eq + std::hash::Hash + fmt::Debug + fmt::Display {
    /// Create an index from a 0-based `Vec` position.
    fn from_usize(idx: usize) -> Self;

    /// Return the 0-based `Vec` position corresponding to this index.
    fn index(self) -> usize;
}

/// A dense vector indexed by typed keys.
#[derive(Debug, Clone)]
pub struct IndexVec<K: Idx, V> {
    raw: Vec<V>,
    _marker: PhantomData<K>,
}

impl<K: Idx, V> IndexVec<K, V> {
    /// Create an empty `IndexVec`.
    pub fn new() -> Self {
        Self {
            raw: Vec::new(),
            _marker: PhantomData,
        }
    }

    /// Create an empty `IndexVec` with the given capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            raw: Vec::with_capacity(capacity),
            _marker: PhantomData,
        }
    }

    /// Append a value and return its typed key.
    ///
    /// The first pushed value receives key `K::new(1)`.
    pub fn push(&mut self, value: V) -> K {
        let idx = self.raw.len();
        self.raw.push(value);
        K::from_usize(idx)
    }

    /// Remove and return the last element, if any.
    pub fn pop(&mut self) -> Option<V> {
        self.raw.pop()
    }

    /// Look up a value by key.
    pub fn get(&self, key: K) -> Option<&V> {
        self.raw.get(key.index())
    }

    /// Look up a value by key mutably.
    pub fn get_mut(&mut self, key: K) -> Option<&mut V> {
        self.raw.get_mut(key.index())
    }

    /// Returns `true` if the key is within the allocated range.
    pub fn contains_key(&self, key: K) -> bool {
        key.index() < self.raw.len()
    }

    /// Return the number of stored values.
    pub fn len(&self) -> usize {
        self.raw.len()
    }

    /// Return whether the vector is empty.
    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }

    /// Iterate over values in key order.
    pub fn iter(&self) -> impl Iterator<Item = &V> {
        self.raw.iter()
    }

    /// Iterate over `(key, value)` pairs in key order.
    pub fn iter_enumerated(&self) -> impl Iterator<Item = (K, &V)> {
        self.raw
            .iter()
            .enumerate()
            .map(|(idx, value)| (K::from_usize(idx), value))
    }

    /// Iterate over keys in order.
    pub fn keys(&self) -> impl Iterator<Item = K> {
        (0..self.raw.len()).map(K::from_usize)
    }

    /// Iterate over values in key order.
    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.raw.iter()
    }

    /// Clear all values. Previously returned keys are now invalid.
    pub fn clear(&mut self) {
        self.raw.clear();
    }
}

impl<K: Idx, V: Default> IndexVec<K, V> {
    /// Grow the underlying vector so that `key` is addressable, filling new
    /// slots with the default value. Returns a mutable reference to the slot.
    pub fn resize_for_key(&mut self, key: K) -> &mut V {
        let idx = key.index();
        if idx >= self.raw.len() {
            self.raw.resize_with(idx + 1, V::default);
        }
        &mut self.raw[idx]
    }

    /// Insert a value at an arbitrary key, growing with default values as
    /// needed. Returns the previous value at that key, if any.
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        let slot = self.resize_for_key(key);
        Some(std::mem::replace(slot, value))
    }
}

impl<K: Idx, V> Default for IndexVec<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Idx, V> Index<K> for IndexVec<K, V> {
    type Output = V;

    fn index(&self, key: K) -> &V {
        let idx = key.index();
        assert!(
            idx < self.raw.len(),
            "IndexVec index out of bounds: {} (len = {})",
            key,
            self.raw.len()
        );
        &self.raw[idx]
    }
}

impl<K: Idx, V> IndexMut<K> for IndexVec<K, V> {
    fn index_mut(&mut self, key: K) -> &mut V {
        let idx = key.index();
        assert!(
            idx < self.raw.len(),
            "IndexVec index out of bounds: {} (len = {})",
            key,
            self.raw.len()
        );
        &mut self.raw[idx]
    }
}

impl<K: Idx, V> IntoIterator for IndexVec<K, V> {
    type Item = V;
    type IntoIter = std::vec::IntoIter<V>;

    fn into_iter(self) -> Self::IntoIter {
        self.raw.into_iter()
    }
}

impl<'a, K: Idx, V> IntoIterator for &'a IndexVec<K, V> {
    type Item = &'a V;
    type IntoIter = std::slice::Iter<'a, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.raw.iter()
    }
}

impl<'a, K: Idx, V> IntoIterator for &'a mut IndexVec<K, V> {
    type Item = &'a mut V;
    type IntoIter = std::slice::IterMut<'a, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.raw.iter_mut()
    }
}

// ----------------------------------------------------------------------------
// SecondaryMap
// ----------------------------------------------------------------------------

/// A dense secondary map keyed by typed indices.
///
/// Backed by `Vec<Option<V>>`, indexed by the same `Idx` type used for an
/// [`IndexVec`]. This is the right tool for per-key metadata when the key space
/// is dense: it has better cache locality and lower per-lookup overhead than
/// `FxHashMap<Idx, V>`.
///
/// For sparse metadata, use `FxHashMap` or a dedicated sparse map instead.
#[derive(Debug, Clone)]
pub struct SecondaryMap<K: Idx, V> {
    raw: Vec<Option<V>>,
    _marker: PhantomData<K>,
}

impl<K: Idx, V> SecondaryMap<K, V> {
    /// Create an empty map.
    pub fn new() -> Self {
        Self {
            raw: Vec::new(),
            _marker: PhantomData,
        }
    }

    /// Create an empty map with the given capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            raw: Vec::with_capacity(capacity),
            _marker: PhantomData,
        }
    }

    /// Ensure the slot for `key` exists, growing with `None` values as needed.
    fn ensure_slot(&mut self, key: K) -> usize {
        let idx = key.index();
        if idx >= self.raw.len() {
            self.raw.resize_with(idx + 1, || None);
        }
        idx
    }

    /// Insert a value and return the previous value at that key, if any.
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        let idx = self.ensure_slot(key);
        self.raw[idx].replace(value)
    }

    /// Look up a value by key.
    pub fn get(&self, key: K) -> Option<&V> {
        self.raw.get(key.index()).and_then(|slot| slot.as_ref())
    }

    /// Look up a value by key mutably.
    pub fn get_mut(&mut self, key: K) -> Option<&mut V> {
        self.raw.get_mut(key.index()).and_then(|slot| slot.as_mut())
    }

    /// Returns `true` if the key has an associated value.
    pub fn contains_key(&self, key: K) -> bool {
        self.get(key).is_some()
    }

    /// Remove and return the value at a key, if any.
    pub fn remove(&mut self, key: K) -> Option<V> {
        self.raw.get_mut(key.index()).and_then(|slot| slot.take())
    }

    /// Return the number of populated entries.
    pub fn len(&self) -> usize {
        self.raw.iter().filter(|slot| slot.is_some()).count()
    }

    /// Return whether the map contains no populated entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Iterate over values in key order.
    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.raw.iter().filter_map(|slot| slot.as_ref())
    }

    /// Iterate over `(key, value)` pairs in key order.
    pub fn iter_enumerated(&self) -> impl Iterator<Item = (K, &V)> {
        self.raw
            .iter()
            .enumerate()
            .filter_map(|(idx, slot)| slot.as_ref().map(|value| (K::from_usize(idx), value)))
    }

    /// Clear all entries. Previously returned keys are now invalid entries.
    pub fn clear(&mut self) {
        self.raw.clear();
    }
}

impl<K: Idx, V> Default for SecondaryMap<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{IndexVec, SecondaryMap};
    use crate::id::Id;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    struct Tag;
    type Key = Id<Tag>;

    #[test]
    fn push_returns_monotonic_one_based_keys() {
        let mut vec: IndexVec<Key, i32> = IndexVec::new();
        let a = vec.push(10);
        let b = vec.push(20);
        let c = vec.push(30);
        assert_eq!(a.raw(), 1);
        assert_eq!(b.raw(), 2);
        assert_eq!(c.raw(), 3);
    }

    #[test]
    fn get_and_index() {
        let mut vec: IndexVec<Key, i32> = IndexVec::new();
        let a = vec.push(10);
        let b = vec.push(20);
        assert_eq!(vec.get(a), Some(&10));
        assert_eq!(vec.get(b), Some(&20));
        assert_eq!(vec[a], 10);
        assert_eq!(vec[b], 20);
    }

    #[test]
    fn get_out_of_bounds_returns_none() {
        let vec: IndexVec<Key, i32> = IndexVec::new();
        assert_eq!(vec.get(Key::new(1)), None);
    }

    #[test]
    fn iter_enumerated_pairs_keys_and_values() {
        let mut vec: IndexVec<Key, i32> = IndexVec::new();
        vec.push(10);
        vec.push(20);
        let pairs: Vec<_> = vec.iter_enumerated().collect();
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].0, Key::new(1));
        assert_eq!(*pairs[0].1, 10);
        assert_eq!(pairs[1].0, Key::new(2));
        assert_eq!(*pairs[1].1, 20);
    }

    #[test]
    fn insert_sparse_grows_with_default() {
        let mut vec: IndexVec<Key, Option<i32>> = IndexVec::new();
        let old = vec.insert(Key::new(3), Some(42));
        assert_eq!(old, Some(None));
        assert_eq!(vec.len(), 3);
        assert_eq!(vec[Key::new(1)], None);
        assert_eq!(vec[Key::new(2)], None);
        assert_eq!(vec[Key::new(3)], Some(42));
    }

    #[test]
    fn insert_returns_previous_value() {
        let mut vec: IndexVec<Key, Option<i32>> = IndexVec::new();
        vec.insert(Key::new(1), Some(10));
        let old = vec.insert(Key::new(1), Some(20));
        assert_eq!(old, Some(Some(10)));
        assert_eq!(vec[Key::new(1)], Some(20));
    }

    #[test]
    #[should_panic(expected = "IndexVec index out of bounds")]
    fn index_panics_on_invalid_key() {
        let vec: IndexVec<Key, i32> = IndexVec::new();
        let _ = vec[Key::new(5)];
    }

    #[test]
    fn secondary_map_insert_and_get() {
        let mut map: SecondaryMap<Key, i32> = SecondaryMap::new();
        assert_eq!(map.get(Key::new(1)), None);
        assert_eq!(map.insert(Key::new(1), 10), None);
        assert_eq!(map.get(Key::new(1)), Some(&10));
        assert_eq!(map.insert(Key::new(1), 20), Some(10));
        assert_eq!(map.get(Key::new(1)), Some(&20));
    }

    #[test]
    fn secondary_map_sparse_growth() {
        let mut map: SecondaryMap<Key, i32> = SecondaryMap::new();
        map.insert(Key::new(3), 42);
        assert_eq!(map.get(Key::new(1)), None);
        assert_eq!(map.get(Key::new(2)), None);
        assert_eq!(map.get(Key::new(3)), Some(&42));
        assert!(!map.contains_key(Key::new(2)));
        assert!(map.contains_key(Key::new(3)));
    }

    #[test]
    fn secondary_map_remove_and_len() {
        let mut map: SecondaryMap<Key, i32> = SecondaryMap::new();
        map.insert(Key::new(1), 10);
        map.insert(Key::new(3), 30);
        assert_eq!(map.len(), 2);
        assert_eq!(map.remove(Key::new(1)), Some(10));
        assert_eq!(map.len(), 1);
        assert_eq!(map.remove(Key::new(2)), None);
    }

    #[test]
    fn secondary_map_iter_enumerated() {
        let mut map: SecondaryMap<Key, i32> = SecondaryMap::new();
        map.insert(Key::new(1), 10);
        map.insert(Key::new(3), 30);
        let pairs: Vec<_> = map.iter_enumerated().collect();
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].0, Key::new(1));
        assert_eq!(*pairs[0].1, 10);
        assert_eq!(pairs[1].0, Key::new(3));
        assert_eq!(*pairs[1].1, 30);
    }
}
