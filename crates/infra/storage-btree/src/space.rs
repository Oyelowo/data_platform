//! Page-id allocation and free-space management.
//!
//! Page IDs are 64-bit integers.  ID 0 is reserved as `NULL_PAGE_ID`.  The
//! allocator hands out never-used IDs monotonically, then reuses IDs from a
//! freelist once pages are freed by the B+ tree.

use crate::page::{NULL_PAGE_ID, PageId};

/// Manages the set of reusable and newly-minted page IDs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PageAllocator {
    next: PageId,
    freelist: Vec<PageId>,
}

impl PageAllocator {
    /// Create an allocator that will hand out IDs starting at `next`.
    pub fn new(next: PageId) -> Self {
        Self {
            next: next.max(NULL_PAGE_ID + 1),
            freelist: Vec::new(),
        }
    }

    /// Allocate a page id.  Reuses a freelisted id if one is available,
    /// otherwise mints a new one.
    pub fn allocate(&mut self) -> PageId {
        if let Some(id) = self.freelist.pop() {
            return id;
        }
        let id = self.next;
        self.next += 1;
        id
    }

    /// Return a page id to the freelist.  `NULL_PAGE_ID` and ids already in
    /// the freelist are ignored.
    pub fn free(&mut self, id: PageId) {
        if id == NULL_PAGE_ID {
            return;
        }
        if self.freelist.contains(&id) {
            return;
        }
        self.freelist.push(id);
    }

    /// Snapshot the allocator state for checkpointing.
    pub fn snapshot(&self) -> (Vec<PageId>, PageId) {
        (self.freelist.clone(), self.next)
    }

    /// Restore allocator state from a checkpoint.
    pub fn restore(&mut self, freelist: Vec<PageId>, next: PageId) {
        self.freelist = freelist;
        self.next = next.max(NULL_PAGE_ID + 1);
    }

    /// Number of ids currently available for reuse.
    pub fn reusable_count(&self) -> usize {
        self.freelist.len()
    }

    /// Next never-used id that would be minted.
    pub fn next_id(&self) -> PageId {
        self.next
    }

    /// Ensure `id` is considered allocated.  If it is in the freelist it is
    /// removed; if it is >= `next`, `next` is advanced past it.  Used by
    /// recovery to resurrect pages that were allocated during forward
    /// processing but never flushed.
    pub fn allocate_specific(&mut self, id: PageId) {
        if id == NULL_PAGE_ID {
            return;
        }
        self.freelist.retain(|&x| x != id);
        if id >= self.next {
            self.next = id + 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocate_mints_ids_monotonically() {
        let mut alloc = PageAllocator::new(1);
        assert_eq!(alloc.allocate(), 1);
        assert_eq!(alloc.allocate(), 2);
        assert_eq!(alloc.allocate(), 3);
    }

    #[test]
    fn freed_ids_are_reused() {
        let mut alloc = PageAllocator::new(1);
        assert_eq!(alloc.allocate(), 1);
        assert_eq!(alloc.allocate(), 2);
        alloc.free(1);
        assert_eq!(alloc.allocate(), 1);
        assert_eq!(alloc.allocate(), 3);
    }

    #[test]
    fn null_id_is_never_allocated_or_freed() {
        let mut alloc = PageAllocator::new(0);
        assert_eq!(alloc.allocate(), 1);
        alloc.free(NULL_PAGE_ID);
        assert_eq!(alloc.reusable_count(), 0);
    }

    #[test]
    fn snapshot_roundtrip() {
        let mut alloc = PageAllocator::new(1);
        let _ = alloc.allocate();
        let _ = alloc.allocate();
        alloc.free(1);
        let (freelist, next) = alloc.snapshot();
        let mut restored = PageAllocator::new(99);
        restored.restore(freelist, next);
        assert_eq!(restored, alloc);
    }
}
