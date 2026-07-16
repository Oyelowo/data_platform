//! Heap-based merging iterator over multiple sorted child iterators.

use crate::Result;

/// Internal iterator trait used by the merge iterator.
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
