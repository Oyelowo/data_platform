//! Global file-number allocator.
//!
//! All column families share a single monotonic file-number space because all
//! SSTables live in the same database directory.  The allocator is cheap to
//! clone (it is just an `Arc<AtomicU64>`) and is safe to share across threads
//! and across column families.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::FileNumber;

/// Thread-safe monotonic allocator for SSTable and metadata file numbers.
#[derive(Clone, Debug)]
pub struct FileNumberAllocator(Arc<AtomicU64>);

impl FileNumberAllocator {
    /// Create an allocator that will hand out `start`, `start + 1`, ...
    pub fn new(start: FileNumber) -> Self {
        Self(Arc::new(AtomicU64::new(start)))
    }

    /// Allocate and return the next file number.
    pub fn next(&self) -> FileNumber {
        self.0.fetch_add(1, Ordering::SeqCst)
    }

    /// Return the next file number that would be allocated without consuming it.
    pub fn current(&self) -> FileNumber {
        self.0.load(Ordering::SeqCst)
    }

    /// Ensure the allocator is at least `n`.  Used during recovery when the
    /// next file number must skip over files already on disk.
    pub fn ensure_at_least(&self, n: FileNumber) {
        self.0.fetch_max(n, Ordering::SeqCst);
    }
}

impl Default for FileNumberAllocator {
    fn default() -> Self {
        Self::new(1)
    }
}
