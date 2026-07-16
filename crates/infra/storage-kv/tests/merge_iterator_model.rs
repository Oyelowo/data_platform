//! Model-based property test for the merge iterator.
//!
//! The merge iterator must produce exactly the same sequence as a reference
//! sort using the internal-key comparator: ascending user key, then descending
//! sequence.  This test generates random overlapping internal keys, splits them
//! into sorted children, and compares the merged output.

use std::collections::HashSet;

use proptest::prelude::*;
use storage_kv::internal_key::{
    ValueType, build_internal_key, compare_internal_keys, extract_user_key,
};
use storage_kv::merge_iter::{InternalIterator, MergeIterator};

/// A test iterator backed by a sorted vector of internal-key entries.
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
    fn seek_to_first(&mut self) -> storage_kv::Result<()> {
        self.pos = 0;
        Ok(())
    }

    fn seek(&mut self, target: &[u8]) -> storage_kv::Result<()> {
        self.pos = self
            .entries
            .partition_point(|(k, _)| extract_user_key(k) < target);
        Ok(())
    }

    fn next(&mut self) -> storage_kv::Result<()> {
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

fn value_type_strategy() -> impl Strategy<Value = ValueType> {
    prop_oneof![Just(ValueType::Value), Just(ValueType::Deletion)]
}

fn entries_strategy() -> impl Strategy<Value = Vec<(Vec<u8>, u64, ValueType, Vec<u8>)>> {
    prop::collection::vec(
        (
            prop::collection::vec(any::<u8>(), 0..8),
            any::<u64>(),
            value_type_strategy(),
            prop::collection::vec(any::<u8>(), 0..16),
        ),
        0..128,
    )
    .prop_filter("unique sequence numbers", |entries| {
        let seqs: HashSet<_> = entries.iter().map(|(_, seq, _, _)| *seq).collect();
        seqs.len() == entries.len()
    })
}

type ChildrenAndEntries = (Vec<Box<dyn InternalIterator>>, Vec<(Vec<u8>, Vec<u8>)>);

fn build_children(
    entries: Vec<(Vec<u8>, u64, ValueType, Vec<u8>)>,
    num_children: usize,
) -> ChildrenAndEntries {
    let mut children: Vec<Vec<(Vec<u8>, Vec<u8>)>> =
        (0..num_children).map(|_| Vec::new()).collect();
    for (idx, (user_key, seq, ty, value)) in entries.into_iter().enumerate() {
        let ikey = build_internal_key(&user_key, seq, ty);
        children[idx % num_children].push((ikey, value));
    }

    for child in &mut children {
        child.sort_by(|a, b| compare_internal_keys(&a.0, &b.0));
    }

    let all: Vec<(Vec<u8>, Vec<u8>)> = children
        .iter()
        .flat_map(|c| c.clone())
        .collect::<Vec<_>>()
        .into_iter()
        .collect();

    let iterators: Vec<Box<dyn InternalIterator>> = children
        .into_iter()
        .map(|c| Box::new(VecIterator::new(c)) as Box<dyn InternalIterator>)
        .collect();

    (iterators, all)
}

proptest! {
    #[test]
    fn merge_iterator_matches_reference_sort(
        entries in entries_strategy(),
        num_children in 1usize..5,
    ) {
        let (children, mut all) = build_children(entries, num_children);
        all.sort_by(|a, b| compare_internal_keys(&a.0, &b.0));

        let mut merge = MergeIterator::new(children).unwrap();
        merge.seek_to_first().unwrap();

        let mut merged = Vec::new();
        while merge.valid() {
            merged.push((merge.key().to_vec(), merge.value().to_vec()));
            merge.next().unwrap();
        }

        prop_assert_eq!(merged, all);
    }

    #[test]
    fn merge_iterator_seek_matches_reference_lower_bound(
        entries in entries_strategy(),
        num_children in 1usize..5,
        target_user_key in prop::collection::vec(any::<u8>(), 0..8),
    ) {
        let (children, mut all) = build_children(entries, num_children);
        all.sort_by(|a, b| compare_internal_keys(&a.0, &b.0));

        let mut merge = MergeIterator::new(children).unwrap();
        merge.seek(&target_user_key).unwrap();

        let mut merged = Vec::new();
        while merge.valid() {
            merged.push((merge.key().to_vec(), merge.value().to_vec()));
            merge.next().unwrap();
        }

        let expected: Vec<_> = all
            .into_iter()
            .skip_while(|(k, _)| extract_user_key(k) < target_user_key.as_slice())
            .collect();

        prop_assert_eq!(merged, expected);
    }
}
