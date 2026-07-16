//! Live snapshot registry.
//!
//! Compaction must preserve the newest version of each key that is visible to
//! every live snapshot.  This module tracks the sequence numbers of all
//! currently active snapshots so compaction can compute the oldest sequence
//! that must be preserved.

use std::collections::BTreeMap;

use crate::SequenceNumber;

/// Tracks live snapshots by sequence number and reference count.
#[derive(Debug, Default)]
pub struct SnapshotRegistry {
    /// sequence number -> number of live snapshots at that sequence.
    snapshots: BTreeMap<SequenceNumber, usize>,
}

impl SnapshotRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            snapshots: BTreeMap::new(),
        }
    }

    /// Register a live snapshot at `seq`.
    pub fn register(&mut self, seq: SequenceNumber) {
        *self.snapshots.entry(seq).or_insert(0) += 1;
    }

    /// Unregister a snapshot at `seq`.
    pub fn unregister(&mut self, seq: SequenceNumber) {
        if let Some(count) = self.snapshots.get_mut(&seq) {
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
        reg.register(5);
        reg.register(10);
        reg.register(10);
        assert_eq!(reg.oldest(), Some(5));
        assert_eq!(reg.len(), 2);

        reg.unregister(5);
        assert_eq!(reg.oldest(), Some(10));
        assert_eq!(reg.len(), 1);

        reg.unregister(10);
        assert_eq!(reg.oldest(), Some(10));
        assert_eq!(reg.len(), 1);

        reg.unregister(10);
        assert_eq!(reg.oldest(), None);
        assert_eq!(reg.len(), 0);
    }
}
