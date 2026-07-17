//! Live snapshot registry.
//!
//! Compaction must preserve the newest version of each key that is visible to
//! every live snapshot.  This module tracks the sequence numbers of all
//! currently active snapshots so compaction can compute the oldest sequence
//! that must be preserved.
//!
//! In addition, each snapshot pins a consistent view of the engine (MemTables
//! plus a `Version`).  The set of blob files referenced by that pinned view is
//! recorded here so blob garbage collection can avoid deleting files that an
//! in-flight snapshot may still read.

use std::collections::{BTreeMap, HashSet};

use crate::{FileNumber, SequenceNumber};

/// Tracks live snapshots by sequence number and reference count.
#[derive(Debug, Default)]
pub struct SnapshotRegistry {
    /// sequence number -> (number of live snapshots at that sequence,
    /// blob files referenced by their pinned view).
    snapshots: BTreeMap<SequenceNumber, (usize, HashSet<FileNumber>)>,
}

impl SnapshotRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            snapshots: BTreeMap::new(),
        }
    }

    /// Register a live snapshot at `seq` that references `blob_files`.
    pub fn register(&mut self, seq: SequenceNumber, blob_files: HashSet<FileNumber>) {
        let entry = self.snapshots.entry(seq).or_insert((0, HashSet::new()));
        entry.0 += 1;
        if entry.1.is_empty() {
            entry.1 = blob_files;
        }
    }

    /// Unregister a snapshot at `seq`.
    pub fn unregister(&mut self, seq: SequenceNumber) {
        if let Some((count, _)) = self.snapshots.get_mut(&seq) {
            *count -= 1;
            if *count == 0 {
                self.snapshots.remove(&seq);
            }
        }
    }

    /// Return the oldest live snapshot sequence, if any.
    pub fn oldest(&self) -> Option<SequenceNumber> {
        self.snapshots.keys().next().copied()
    }

    /// Return every live snapshot sequence.
    pub fn all(&self) -> Vec<SequenceNumber> {
        self.snapshots.keys().copied().collect()
    }

    /// Return the union of blob file numbers referenced by all live snapshots.
    #[allow(dead_code)]
    pub fn blob_files(&self) -> HashSet<FileNumber> {
        self.snapshots
            .values()
            .flat_map(|(_, files)| files.iter().copied())
            .collect()
    }

    /// Number of distinct live snapshot sequences.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.snapshots.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_unregister() {
        let mut reg = SnapshotRegistry::new();
        reg.register(5, [1].into());
        reg.register(10, [2].into());
        reg.register(10, [2].into());
        assert_eq!(reg.oldest(), Some(5));
        assert_eq!(reg.len(), 2);
        assert!(reg.blob_files().contains(&1));
        assert!(reg.blob_files().contains(&2));

        reg.unregister(5);
        assert_eq!(reg.oldest(), Some(10));
        assert_eq!(reg.len(), 1);
        assert!(!reg.blob_files().contains(&1));

        reg.unregister(10);
        assert_eq!(reg.oldest(), Some(10));
        assert_eq!(reg.len(), 1);

        reg.unregister(10);
        assert_eq!(reg.oldest(), None);
        assert_eq!(reg.len(), 0);
        assert!(reg.blob_files().is_empty());
    }
}
