//! Heap-based merging iterator over multiple sorted child iterators.
//!
//! The children are ordered by internal key: ascending user key, descending
//! sequence number. This matches the ordering produced by MemTables and
//! SSTables, so the merge iterator naturally surfaces the newest version of
//! each user key first.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use crate::Result;
use crate::internal_key::compare_internal_keys;

/// Internal iterator trait used by the merge iterator.
///
/// Keys are full internal keys (user key + sequence + type). Values are the
/// raw value bytes.
pub trait InternalIterator {
    /// Position at the first entry.
    fn seek_to_first(&mut self) -> Result<()>;
    /// Position at the first entry with key >= target.
    fn seek(&mut self, target: &[u8]) -> Result<()>;
    /// Advance to the next entry.
    fn next(&mut self) -> Result<()>;
    /// True if positioned at a valid entry.
    fn valid(&self) -> bool;
    /// Current internal key.
    fn key(&self) -> &[u8];
    /// Current value.
    fn value(&self) -> &[u8];
}

/// A heap-based k-way merge iterator.
pub struct MergeIterator {
    children: Vec<Box<dyn InternalIterator>>,
    heap: BinaryHeap<HeapEntry>,
}

impl MergeIterator {
    /// Create a merge iterator from a set of children. Each child is seeked to
    /// its first entry.
    pub fn new(mut children: Vec<Box<dyn InternalIterator>>) -> Result<Self> {
        let mut heap = BinaryHeap::with_capacity(children.len());
        for (i, child) in children.iter_mut().enumerate() {
            child.seek_to_first()?;
            if child.valid() {
                heap.push(HeapEntry {
                    key: child.key().to_vec(),
                    child_index: i,
                });
            }
        }
        Ok(Self { children, heap })
    }

    /// Seek all children to their first entry and rebuild the heap.
    pub fn seek_to_first(&mut self) -> Result<()> {
        self.heap.clear();
        for (i, child) in self.children.iter_mut().enumerate() {
            child.seek_to_first()?;
            if child.valid() {
                self.heap.push(HeapEntry {
                    key: child.key().to_vec(),
                    child_index: i,
                });
            }
        }
        Ok(())
    }

    /// Seek all children to `target` and rebuild the heap.
    pub fn seek(&mut self, target: &[u8]) -> Result<()> {
        self.heap.clear();
        for (i, child) in self.children.iter_mut().enumerate() {
            child.seek(target)?;
            if child.valid() {
                self.heap.push(HeapEntry {
                    key: child.key().to_vec(),
                    child_index: i,
                });
            }
        }
        Ok(())
    }

    /// Advance the iterator to the next entry.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<()> {
        let entry = self
            .heap
            .pop()
            .ok_or_else(|| crate::Error::InvalidArgument("iterator is not valid".into()))?;
        let child = &mut self.children[entry.child_index];
        child.next()?;
        if child.valid() {
            self.heap.push(HeapEntry {
                key: child.key().to_vec(),
                child_index: entry.child_index,
            });
        }
        Ok(())
    }

    /// True if positioned at a valid entry.
    pub fn valid(&self) -> bool {
        !self.heap.is_empty()
    }

    /// Current internal key.
    pub fn key(&self) -> &[u8] {
        &self.heap.peek().expect("invalid iterator").key
    }

    /// Current value.
    pub fn value(&self) -> &[u8] {
        let idx = self.heap.peek().expect("invalid iterator").child_index;
        self.children[idx].value()
    }
}

struct HeapEntry {
    key: Vec<u8>,
    child_index: usize,
}

impl Ord for HeapEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // `BinaryHeap` is a max-heap, so we invert the comparator to obtain a
        // min-heap ordered by internal key.
        compare_internal_keys(&other.key, &self.key)
    }
}

impl PartialOrd for HeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for HeapEntry {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl Eq for HeapEntry {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal_key::{ValueType, build_internal_key};

    struct VecIterator {
        entries: Vec<(Vec<u8>, Vec<u8>)>,
        pos: usize,
    }

    impl VecIterator {
        fn new(entries: Vec<(Vec<u8>, Vec<u8>)>) -> Self {
            Self { entries, pos: 0 }
        }
    }

    impl InternalIterator for VecIterator {
        fn seek_to_first(&mut self) -> Result<()> {
            self.pos = 0;
            Ok(())
        }

        fn seek(&mut self, target: &[u8]) -> Result<()> {
            self.pos = self.entries.partition_point(|(k, _)| k.as_slice() < target);
            Ok(())
        }

        fn next(&mut self) -> Result<()> {
            if self.pos < self.entries.len() {
                self.pos += 1;
            }
            Ok(())
        }

        fn valid(&self) -> bool {
            self.pos < self.entries.len()
        }

        fn key(&self) -> &[u8] {
            &self.entries[self.pos].0
        }

        fn value(&self) -> &[u8] {
            &self.entries[self.pos].1
        }
    }

    #[test]
    fn merge_empty() {
        let iter = MergeIterator::new(Vec::new()).unwrap();
        assert!(!iter.valid());
    }

    #[test]
    fn merge_disjoint() {
        let children: Vec<Box<dyn InternalIterator>> = vec![
            Box::new(VecIterator::new(vec![
                (b"a".to_vec(), b"1".to_vec()),
                (b"c".to_vec(), b"2".to_vec()),
            ])),
            Box::new(VecIterator::new(vec![
                (b"b".to_vec(), b"3".to_vec()),
                (b"d".to_vec(), b"4".to_vec()),
            ])),
        ];
        let mut iter = MergeIterator::new(children).unwrap();
        iter.seek_to_first().unwrap();
        assert_eq!(iter.key(), b"a");
        iter.next().unwrap();
        assert_eq!(iter.key(), b"b");
        iter.next().unwrap();
        assert_eq!(iter.key(), b"c");
        iter.next().unwrap();
        assert_eq!(iter.key(), b"d");
        iter.next().unwrap();
        assert!(!iter.valid());
    }

    #[test]
    fn merge_overlapping_internal_keys() {
        // Same user key, different sequence numbers. Internal key order puts
        // the larger sequence first.
        let children: Vec<Box<dyn InternalIterator>> = vec![
            Box::new(VecIterator::new(vec![(
                build_internal_key(b"k", 5, ValueType::Value),
                b"old".to_vec(),
            )])),
            Box::new(VecIterator::new(vec![(
                build_internal_key(b"k", 6, ValueType::Value),
                b"new".to_vec(),
            )])),
        ];
        let mut iter = MergeIterator::new(children).unwrap();
        iter.seek_to_first().unwrap();
        assert_eq!(iter.value(), b"new");
        iter.next().unwrap();
        assert_eq!(iter.value(), b"old");
        iter.next().unwrap();
        assert!(!iter.valid());
    }

    #[test]
    fn merge_seek() {
        let children: Vec<Box<dyn InternalIterator>> = vec![
            Box::new(VecIterator::new(vec![
                (b"a".to_vec(), b"1".to_vec()),
                (b"c".to_vec(), b"2".to_vec()),
            ])),
            Box::new(VecIterator::new(vec![
                (b"b".to_vec(), b"3".to_vec()),
                (b"d".to_vec(), b"4".to_vec()),
            ])),
        ];
        let mut iter = MergeIterator::new(children).unwrap();
        iter.seek(b"c").unwrap();
        assert_eq!(iter.key(), b"c");
        iter.next().unwrap();
        assert_eq!(iter.key(), b"d");
    }
}
