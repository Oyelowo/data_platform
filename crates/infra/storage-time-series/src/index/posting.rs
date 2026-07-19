//! Posting-list helpers for tag filters.

use std::collections::BTreeSet;

/// Set operation on posting lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostingOp {
    /// Union.
    Union,
    /// Intersection.
    Intersect,
}

/// Intersect an accumulator with another set, modifying in place.
pub fn intersect(acc: &mut BTreeSet<Vec<u8>>, other: &BTreeSet<Vec<u8>>) {
    let mut to_remove = Vec::new();
    for item in acc.iter() {
        if !other.contains(item) {
            to_remove.push(item.clone());
        }
    }
    for item in to_remove {
        acc.remove(&item);
    }
}

/// Union an accumulator with another set, modifying in place.
pub fn union(acc: &mut BTreeSet<Vec<u8>>, other: &BTreeSet<Vec<u8>>) {
    acc.extend(other.iter().cloned());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intersect_and_union() {
        let mut a: BTreeSet<Vec<u8>> = [b"a".to_vec(), b"b".to_vec()].into_iter().collect();
        let b: BTreeSet<Vec<u8>> = [b"b".to_vec(), b"c".to_vec()].into_iter().collect();
        intersect(&mut a, &b);
        assert_eq!(a.len(), 1);
        assert!(a.contains(b"b".as_slice()));
        union(&mut a, &b);
        assert_eq!(a.len(), 2);
        assert!(a.contains(b"b".as_slice()));
        assert!(a.contains(b"c".as_slice()));
    }
}
