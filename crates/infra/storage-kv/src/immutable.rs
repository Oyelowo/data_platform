//! Immutable MemTable queue and SSTable path helpers.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::memtable::MemTable;
use crate::FileNumber;

/// Path for an SSTable file.
pub fn sstable_path(db_path: &Path, number: FileNumber) -> PathBuf {
    db_path.join(format!("{:06}.sst", number))
}

/// A queue of frozen MemTables waiting to be flushed to disk.
///
/// The queue preserves insertion order: the oldest MemTable is at the front
/// and is flushed first. Reads search the queue from newest to oldest.
#[derive(Default)]
pub struct ImmutableMemTables {
    tables: Vec<Arc<MemTable>>,
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
    pub fn push(&mut self, table: Arc<MemTable>) {
        self.tables.push(table);
    }

    /// Pop the oldest MemTable from the front of the queue.
    pub fn pop(&mut self) -> Option<Arc<MemTable>> {
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

    /// Iterate from newest (back) to oldest (front).
    pub fn iter_newest_first(&self) -> impl Iterator<Item = &Arc<MemTable>> {
        self.tables.iter().rev()
    }

    /// Approximate total byte size of all queued MemTables.
    #[allow(dead_code)]
    pub fn approximate_size(&self) -> usize {
        self.tables.iter().map(|t| t.approximate_size()).sum()
    }
}
