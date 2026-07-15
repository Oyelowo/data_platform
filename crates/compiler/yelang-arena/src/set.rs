use rustc_hash::FxHashSet as InnerFxHashSet;
use std::hash::Hash;

/// A fast hash set using FxHash.
///
/// Wrapper around `rustc_hash::FxHashSet`.
#[derive(Debug, Clone)]
pub struct FxHashSet<T> {
    inner: InnerFxHashSet<T>,
}

impl<T> FxHashSet<T>
where
    T: Eq + Hash,
{
    pub fn new() -> Self {
        Self {
            inner: InnerFxHashSet::default(),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: InnerFxHashSet::with_capacity_and_hasher(capacity, Default::default()),
        }
    }

    pub fn insert(&mut self, value: T) -> bool {
        self.inner.insert(value)
    }

    pub fn contains<Q>(&self, value: &Q) -> bool
    where
        T: std::borrow::Borrow<Q>,
        Q: Eq + Hash,
    {
        self.inner.contains(value)
    }

    pub fn remove<Q>(&mut self, value: &Q) -> bool
    where
        T: std::borrow::Borrow<Q>,
        Q: Eq + Hash,
    {
        self.inner.remove(value)
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.inner.iter()
    }

    pub fn clear(&mut self) {
        self.inner.clear();
    }

    pub fn reserve(&mut self, additional: usize) {
        self.inner.reserve(additional);
    }

    pub fn take(&mut self, value: &T) -> Option<T> {
        self.inner.take(value)
    }

    pub fn get<Q>(&self, value: &Q) -> Option<&T>
    where
        T: std::borrow::Borrow<Q>,
        Q: Eq + Hash,
    {
        self.inner.get(value)
    }
}

impl<T> Default for FxHashSet<T>
where
    T: Eq + Hash,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<T> IntoIterator for FxHashSet<T> {
    type Item = T;
    type IntoIter = std::collections::hash_set::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a FxHashSet<T> {
    type Item = &'a T;
    type IntoIter = std::collections::hash_set::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter()
    }
}

impl<T> FromIterator<T> for FxHashSet<T>
where
    T: Eq + Hash,
{
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut set = Self::new();
        for item in iter {
            set.insert(item);
        }
        set
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fx_set_basic() {
        let mut set = FxHashSet::new();
        set.insert(1);
        set.insert(2);
        assert!(set.contains(&1));
        assert!(!set.contains(&3));
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn fx_set_from_iter() {
        let set: FxHashSet<i32> = [1, 2, 2, 3].into_iter().collect();
        assert_eq!(set.len(), 3);
    }
}
