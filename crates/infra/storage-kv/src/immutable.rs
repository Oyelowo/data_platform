//! Immutable MemTable queue and SSTable path helpers.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::FileNumber;
use crate::memtable::MemTable;

/// Path for an SSTable file.
pub fn sstable_path(db_path: &Path, number: FileNumber) -> PathBuf {
    db_path.join(format!("{:06}.sst", number))
}

/// A queue of frozen MemTables waiting to be flushed to disk.
///
/// Each MemTable is paired with the L0 file number reserved for it at freeze
/// time.  Reserving the number when the table is frozen — rather than when
/// the flush runs — guarantees that file-number order always matches version
/// order, even when the background worker and the synchronous backpressure
/// flush path interleave.
///
/// The queue preserves insertion order: the oldest MemTable is at the front
/// and is flushed first.  Reads search the queue from newest to oldest, so
/// [`ImmutableMemTables::snapshot`] returns the tables newest first.
#[derive(Default)]
pub struct ImmutableMemTables {
    tables: Vec<(FileNumber, Arc<MemTable>)>,
    max_size: usize,
}

impl ImmutableMemTables {
    /// Create an empty queue that stalls writes when it reaches `max_size`.
    pub fn new(max_size: usize) -> Self {
        Self {
            tables: Vec::new(),
            max_size,
        }
    }

    /// Push a newly frozen MemTable to the back of the queue.
    pub fn push(&mut self, number: FileNumber, table: Arc<MemTable>) {
        self.tables.push((number, table));
    }

    /// Return a clone of the oldest MemTable without removing it.
    pub fn front(&self) -> Option<(FileNumber, Arc<MemTable>)> {
        self.tables.first().cloned()
    }

    /// Pop the oldest MemTable from the front of the queue.
    pub fn pop(&mut self) -> Option<(FileNumber, Arc<MemTable>)> {
        if self.tables.is_empty() {
            return None;
        }
        Some(self.tables.remove(0))
    }

    /// Number of immutable MemTables queued.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.tables.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tables.is_empty()
    }

    /// True if the queue is full and writers should stall.
    pub fn is_full(&self) -> bool {
        self.tables.len() >= self.max_size
    }

    /// Approximate total byte size of all queued MemTables.
    #[allow(dead_code)]
    pub fn approximate_size(&self) -> usize {
        self.tables.iter().map(|(_, t)| t.approximate_size()).sum()
    }

    /// Return a shallow clone of the queued tables as a snapshot, **newest
    /// first**.
    ///
    /// Point reads search the tables in order and stop at the first hit, so
    /// the newest table — which holds the newest versions — must come first.
    /// The returned vector shares the `Arc`s with the engine, so readers can
    /// hold the tables without keeping the engine lock.
    pub fn snapshot(&self) -> Vec<Arc<MemTable>> {
        self.tables
            .iter()
            .rev()
            .map(|(_, t)| Arc::clone(t))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_is_newest_first() {
        let mut queue = ImmutableMemTables::new(3);
        let t1 = Arc::new(MemTable::new());
        let t2 = Arc::new(MemTable::new());
        let t3 = Arc::new(MemTable::new());
        queue.push(10, Arc::clone(&t1));
        queue.push(11, Arc::clone(&t2));
        queue.push(12, Arc::clone(&t3));

        let snap = queue.snapshot();
        assert_eq!(snap.len(), 3);
        assert!(Arc::ptr_eq(&snap[0], &t3));
        assert!(Arc::ptr_eq(&snap[1], &t2));
        assert!(Arc::ptr_eq(&snap[2], &t1));
    }

    #[test]
    fn front_and_pop_return_oldest_with_number() {
        let mut queue = ImmutableMemTables::new(3);
        let t1 = Arc::new(MemTable::new());
        let t2 = Arc::new(MemTable::new());
        queue.push(10, Arc::clone(&t1));
        queue.push(11, Arc::clone(&t2));

        let (n, front) = queue.front().unwrap();
        assert_eq!(n, 10);
        assert!(Arc::ptr_eq(&front, &t1));

        let (n, popped) = queue.pop().unwrap();
        assert_eq!(n, 10);
        assert!(Arc::ptr_eq(&popped, &t1));

        let (n, front) = queue.front().unwrap();
        assert_eq!(n, 11);
        assert!(Arc::ptr_eq(&front, &t2));
    }

    #[test]
    fn full_and_empty() {
        let mut queue = ImmutableMemTables::new(1);
        assert!(queue.is_empty());
        assert!(!queue.is_full());
        queue.push(10, Arc::new(MemTable::new()));
        assert!(!queue.is_empty());
        assert!(queue.is_full());
        queue.pop();
        assert!(queue.is_empty());
    }
}
