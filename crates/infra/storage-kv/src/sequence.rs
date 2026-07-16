//! Sequence-number allocation and watermark publishing.
//!
//! The LSM engine uses monotonically increasing [`SequenceNumber`]s as logical
//! timestamps for every write.  A *snapshot* with sequence `S` must see exactly
//! the writes whose sequence numbers are `<= S` and that have been inserted
//! into a MemTable.
//!
//! To allow fully concurrent writes the allocator hands out sequence numbers
//! without any engine lock.  Each writer is responsible for calling
//! [`SequenceAllocator::release`] once its write is either inserted into the
//! current MemTable or permanently aborted (e.g. WAL append failed).  The
//! allocator tracks in-flight sequences and publishes a *completed watermark*:
//! the highest contiguous sequence number such that every sequence `<=` it has
//! either been inserted or aborted.  Readers use this watermark as their
//! snapshot, which guarantees a consistent view even when writers complete out
//! of order.
//!
//! A sequence number that is allocated but never released would pin the
//! watermark forever.  [`SeqGuard`] makes release panic-safe: the guard releases
//! the sequence in its `Drop` impl unless the writer explicitly calls
//! [`SeqGuard::release`] first.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::SequenceNumber;

/// Thread-safe sequence allocator and watermark publisher.
///
/// The type is cheap to clone: all state lives behind `Arc`s, so clones just
/// increment reference counts.
#[derive(Clone, Debug)]
pub struct SequenceAllocator {
    /// Next sequence number to hand out.  `fetch_add` is lock-free.
    next: Arc<AtomicU64>,
    /// Sequences that have been allocated but not yet released.  Protected by a
    /// mutex because the set is tiny (bounded by the number of concurrent
    /// writers) and a lock-free ordered set would be overkill here.
    pending: Arc<Mutex<BTreeSet<SequenceNumber>>>,
    /// Highest contiguous completed sequence number.  This is the published
    /// snapshot readers observe.
    completed: Arc<AtomicU64>,
}

impl SequenceAllocator {
    /// Create an allocator whose next sequence is `last_used + 1`.
    ///
    /// `last_used` is the largest sequence number already present on disk; for
    /// a fresh engine it should be `0`.
    pub fn new(last_used: SequenceNumber) -> Self {
        Self {
            next: Arc::new(AtomicU64::new(last_used.wrapping_add(1))),
            pending: Arc::new(Mutex::new(BTreeSet::new())),
            completed: Arc::new(AtomicU64::new(last_used)),
        }
    }

    /// Allocate and reserve a new sequence number.
    ///
    /// The caller *must* eventually call [`release`] or drop a [`SeqGuard`]
    /// for the returned sequence.
    pub fn next(&self) -> SequenceNumber {
        let seq = self.next.fetch_add(1, Ordering::Relaxed);
        let mut pending = self.pending.lock().unwrap();
        pending.insert(seq);
        seq
    }

    /// Release a sequence, advancing the published watermark if possible.
    ///
    /// This must be called exactly once per allocated sequence.  It is safe to
    /// call for a sequence whose WAL append failed: such a sequence creates a
    /// harmless gap in the sequence space and the watermark can advance past
    /// it because there is no data associated with it.
    ///
    /// Returns the new published watermark after this release.
    pub fn release(&self, seq: SequenceNumber) -> SequenceNumber {
        let new_completed = {
            let mut pending = self.pending.lock().unwrap();
            pending.remove(&seq);
            // The watermark can advance to just before the lowest pending
            // sequence.  If nothing is pending, advance to the most recently
            // allocated sequence.
            pending
                .iter()
                .next()
                .copied()
                .map(|min| min.wrapping_sub(1))
                .unwrap_or_else(|| self.next.load(Ordering::Acquire).wrapping_sub(1))
        };

        // Try to publish the new watermark.  Multiple writers may race here;
        // only the one that observed the highest value succeeds in moving it
        // forward.
        loop {
            let current = self.completed.load(Ordering::Acquire);
            if new_completed <= current {
                return current;
            }
            if self
                .completed
                .compare_exchange_weak(current, new_completed, Ordering::Release, Ordering::Acquire)
                .is_ok()
            {
                return new_completed;
            }
        }
    }

    /// The largest sequence number that has been allocated so far.
    ///
    /// This is *not* safe to use as a reader snapshot because some lower
    /// sequences may still be in flight.  Use [`completed`] for snapshots.
    pub fn current(&self) -> SequenceNumber {
        self.next.load(Ordering::Acquire).wrapping_sub(1)
    }

    /// The published snapshot sequence number.
    ///
    /// All writes with sequence `<=` this value have either been inserted into a
    /// MemTable or permanently aborted.  Readers may treat this as a consistent
    /// snapshot.
    pub fn completed(&self) -> SequenceNumber {
        self.completed.load(Ordering::Acquire)
    }

    /// True when every allocated sequence has been released.
    pub fn is_quiesced(&self) -> bool {
        self.pending.lock().unwrap().is_empty()
    }

    /// Create a guard that releases `seq` on drop.
    pub fn guard(&self, seq: SequenceNumber) -> SeqGuard<'_> {
        SeqGuard {
            allocator: self,
            seq,
            released: false,
        }
    }
}

/// RAII guard that releases a sequence number when dropped.
///
/// Call [`SeqGuard::release`] once the write is durable and visible to
/// prevent the `Drop` impl from releasing it again.
pub struct SeqGuard<'a> {
    allocator: &'a SequenceAllocator,
    seq: SequenceNumber,
    released: bool,
}

impl<'a> SeqGuard<'a> {
    /// Mark the sequence as released and advance the watermark.
    ///
    /// After calling this the guard must not be used again, but dropping it is
    /// harmless.
    pub fn release(mut self) -> SequenceNumber {
        self.released = true;
        self.allocator.release(self.seq)
    }
}

impl<'a> Drop for SeqGuard<'a> {
    fn drop(&mut self) {
        if !self.released {
            self.allocator.release(self.seq);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocation_is_monotonic() {
        let alloc = SequenceAllocator::new(0);
        assert_eq!(alloc.next(), 1);
        assert_eq!(alloc.next(), 2);
        assert_eq!(alloc.next(), 3);
    }

    #[test]
    fn watermark_advances_in_order() {
        let alloc = SequenceAllocator::new(0);
        let s1 = alloc.next();
        let s2 = alloc.next();
        let s3 = alloc.next();

        assert_eq!(alloc.completed(), 0);

        alloc.release(s2);
        // s1 is still pending, so watermark cannot advance past 0.
        assert_eq!(alloc.completed(), 0);

        alloc.release(s1);
        // Now s1 and s2 are done; s3 is pending.  Watermark = 2.
        assert_eq!(alloc.completed(), 2);

        alloc.release(s3);
        assert_eq!(alloc.completed(), 3);
    }

    #[test]
    fn release_out_of_order_fills_gaps() {
        let alloc = SequenceAllocator::new(0);
        let s1 = alloc.next();
        let s2 = alloc.next();
        let s3 = alloc.next();

        alloc.release(s3);
        assert_eq!(alloc.completed(), 0);

        alloc.release(s1);
        assert_eq!(alloc.completed(), 1);

        alloc.release(s2);
        assert_eq!(alloc.completed(), 3);
    }

    #[test]
    fn guard_releases_on_drop() {
        let alloc = SequenceAllocator::new(0);
        let s = alloc.next();
        {
            let guard = alloc.guard(s);
            assert_eq!(alloc.completed(), 0);
            drop(guard);
        }
        assert_eq!(alloc.completed(), 1);
    }

    #[test]
    fn guard_explicit_release() {
        let alloc = SequenceAllocator::new(0);
        let s = alloc.next();
        let guard = alloc.guard(s);
        assert_eq!(guard.release(), 1);
        assert_eq!(alloc.completed(), 1);
    }

    #[test]
    fn quiesced_when_all_released() {
        let alloc = SequenceAllocator::new(0);
        let s = alloc.next();
        assert!(!alloc.is_quiesced());
        alloc.release(s);
        assert!(alloc.is_quiesced());
    }
}
