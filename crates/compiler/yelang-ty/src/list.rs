/*! Interned list slices.
 *
 * Lists of generic arguments, bound variables, and other type-system
 * sequences are interned so that pointer equality implies structural
 * equality.
 */

use std::fmt;
use std::hash::{Hash, Hasher};

/// An interned, immutable slice.
///
/// `List<T>` is a thin wrapper around a raw slice pointer. It is `Copy`
/// when `T: Copy`. Two lists are equal iff they point to the same
/// interned allocation.
#[derive(Clone, Copy)]
pub struct List<T: Copy> {
    ptr: *const T,
    len: usize,
}

impl<T: Copy> List<T> {
    /// Create a `List` from a slice.
    ///
    /// # Safety
    /// The slice must live at least as long as any `List` constructed
    /// from it. In practice this means the slice must be arena-allocated.
    pub const fn from_slice(slice: &[T]) -> Self {
        Self {
            ptr: slice.as_ptr(),
            len: slice.len(),
        }
    }

    pub const fn empty() -> Self {
        Self {
            ptr: std::ptr::NonNull::dangling().as_ptr(),
            len: 0,
        }
    }

    pub fn as_slice(&self) -> &[T] {
        if self.len == 0 {
            return &[];
        }
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn get(&self, index: usize) -> Option<&T> {
        self.as_slice().get(index)
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.as_slice().iter()
    }
}

impl<T: Copy + fmt::Debug> fmt::Debug for List<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<T: Copy + PartialEq> PartialEq for List<T> {
    fn eq(&self, other: &Self) -> bool {
        // Pointer equality: interning guarantees structural equality.
        self.ptr == other.ptr && self.len == other.len
    }
}

impl<T: Copy + Eq> Eq for List<T> {}

impl<T: Copy + Hash> Hash for List<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Hash the pointer, not the contents.
        self.ptr.hash(state);
        self.len.hash(state);
    }
}

impl<T: Copy> Default for List<T> {
    fn default() -> Self {
        Self::empty()
    }
}

impl<T: Copy> std::ops::Deref for List<T> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        self.as_slice()
    }
}

impl<'a, T: Copy> IntoIterator for &'a List<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.as_slice().iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_list() {
        let list: List<i32> = List::empty();
        assert!(list.is_empty());
        assert_eq!(list.len(), 0);
        assert_eq!(list.as_slice(), &[]);
    }

    #[test]
    fn list_from_slice() {
        let data = [1, 2, 3];
        let list = List::from_slice(&data);
        assert_eq!(list.len(), 3);
        assert_eq!(list.as_slice(), &[1, 2, 3]);
        assert_eq!(list.get(1), Some(&2));
        assert_eq!(list.get(3), None);
    }

    #[test]
    fn list_equality_by_pointer() {
        let data1 = [1, 2, 3];
        let data2 = [1, 2, 3];
        let list1 = List::from_slice(&data1);
        let list2 = List::from_slice(&data1); // same base allocation
        let list3 = List::from_slice(&data2); // different allocation

        assert_eq!(list1, list2);
        // list1 and list3 have same contents but different pointers.
        // This is intentional: interning is required for pointer equality.
        assert_ne!(list1.ptr, list3.ptr);
        assert_ne!(list1, list3);
    }

    #[test]
    fn list_iteration() {
        let data = [10, 20, 30];
        let list = List::from_slice(&data);
        let collected: Vec<i32> = list.iter().copied().collect();
        assert_eq!(collected, vec![10, 20, 30]);
    }
}
