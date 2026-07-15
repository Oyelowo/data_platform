use rustc_hash::FxHashMap as InnerFxHashMap;
use std::hash::Hash;

/// A fast hash map using FxHash.
///
/// Wrapper around `rustc_hash::FxHashMap`.
/// We use this for all compiler-internal maps where the keys are DefIds,
/// HirIds, or other internal identifiers.
#[derive(Debug, Clone)]
pub struct FxHashMap<K, V> {
    inner: InnerFxHashMap<K, V>,
}

impl<K, V> FxHashMap<K, V>
where
    K: Eq + Hash,
{
    pub fn new() -> Self {
        Self {
            inner: InnerFxHashMap::default(),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: InnerFxHashMap::with_capacity_and_hasher(capacity, Default::default()),
        }
    }

    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        self.inner.insert(key, value)
    }

    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: std::borrow::Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.inner.get(key)
    }

    pub fn get_mut<Q>(&mut self, key: &Q) -> Option<&mut V>
    where
        K: std::borrow::Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.inner.get_mut(key)
    }

    pub fn contains_key<Q>(&self, key: &Q) -> bool
    where
        K: std::borrow::Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.inner.contains_key(key)
    }

    pub fn remove<Q>(&mut self, key: &Q) -> Option<V>
    where
        K: std::borrow::Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.inner.remove(key)
    }

    pub fn entry(&mut self, key: K) -> std::collections::hash_map::Entry<'_, K, V> {
        self.inner.entry(key)
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.inner.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&K, &mut V)> {
        self.inner.iter_mut()
    }

    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.inner.keys()
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.inner.values()
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.inner.values_mut()
    }

    pub fn reserve(&mut self, additional: usize) {
        self.inner.reserve(additional);
    }

    pub fn clear(&mut self) {
        self.inner.clear();
    }
}

impl<K, V> Default for FxHashMap<K, V>
where
    K: Eq + Hash,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<K, V> IntoIterator for FxHashMap<K, V> {
    type Item = (K, V);
    type IntoIter = std::collections::hash_map::IntoIter<K, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.into_iter()
    }
}

impl<'a, K, V> IntoIterator for &'a FxHashMap<K, V> {
    type Item = (&'a K, &'a V);
    type IntoIter = std::collections::hash_map::Iter<'a, K, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter()
    }
}

impl<'a, K, V> IntoIterator for &'a mut FxHashMap<K, V> {
    type Item = (&'a K, &'a mut V);
    type IntoIter = std::collections::hash_map::IterMut<'a, K, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter_mut()
    }
}

// ----------------------------------------------------------------------------
// OrderedIndexMap
// ----------------------------------------------------------------------------

use indexmap::IndexMap as InnerIndexMap;

/// An ordered hash map that preserves insertion order.
///
/// Used when deterministic iteration order is required (e.g., diagnostic
/// output, codegen).
#[derive(Debug, Clone)]
pub struct OrderedMap<K, V> {
    inner: InnerIndexMap<K, V>,
}

impl<K, V> OrderedMap<K, V>
where
    K: Eq + Hash,
{
    pub fn new() -> Self {
        Self {
            inner: InnerIndexMap::new(),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: InnerIndexMap::with_capacity(capacity),
        }
    }

    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        self.inner.insert(key, value)
    }

    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: std::borrow::Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.inner.get(key)
    }

    pub fn get_mut<Q>(&mut self, key: &Q) -> Option<&mut V>
    where
        K: std::borrow::Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.inner.get_mut(key)
    }

    pub fn contains_key<Q>(&self, key: &Q) -> bool
    where
        K: std::borrow::Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.inner.contains_key(key)
    }

    pub fn remove<Q>(&mut self, key: &Q) -> Option<V>
    where
        K: std::borrow::Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.inner.shift_remove(key)
    }

    pub fn entry(&mut self, key: K) -> indexmap::map::Entry<'_, K, V> {
        self.inner.entry(key)
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.inner.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&K, &mut V)> {
        self.inner.iter_mut()
    }

    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.inner.keys()
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.inner.values()
    }

    pub fn get_index(&self, index: usize) -> Option<(&K, &V)> {
        self.inner.get_index(index)
    }

    pub fn clear(&mut self) {
        self.inner.clear();
    }

    pub fn reserve(&mut self, additional: usize) {
        self.inner.reserve(additional);
    }
}

impl<K, V> Default for OrderedMap<K, V>
where
    K: Eq + Hash,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<K, V> IntoIterator for OrderedMap<K, V> {
    type Item = (K, V);
    type IntoIter = indexmap::map::IntoIter<K, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.into_iter()
    }
}

impl<'a, K, V> IntoIterator for &'a OrderedMap<K, V> {
    type Item = (&'a K, &'a V);
    type IntoIter = indexmap::map::Iter<'a, K, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fx_map_basic() {
        let mut map = FxHashMap::new();
        map.insert("a", 1);
        map.insert("b", 2);
        assert_eq!(map.get("a"), Some(&1));
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn ordered_map_preserves_order() {
        let mut map = OrderedMap::new();
        map.insert("z", 1);
        map.insert("a", 2);
        let keys: Vec<_> = map.keys().collect();
        assert_eq!(keys, vec![&"z", &"a"]);
    }
}
