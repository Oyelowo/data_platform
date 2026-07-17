//! Mapping table: PID -> page state pointer.
//!
//! The table is sharded so concurrent reads do not contend for a single lock.
//! Each shard is a `Vec<AtomicPtr<PageState>>` protected by an `RwLock`. The
//! lock is taken only for resizing and PID freeing; read and CAS paths hold
//! the read lock for the brief index lookup.
//!
//! # PID reuse
//!
//! Freed PIDs are **not reused** in the first version. This avoids the ABA
//! problem entirely at the cost of unbounded PID growth. A production
//! implementation would delay reuse until the freeing epoch has drained.

use std::ptr;
use std::sync::atomic::{AtomicPtr, AtomicU64, Ordering};

use crossbeam_epoch::{self as epoch};
use parking_lot::RwLock;

use crate::page::{PageState, Pid};

const NUM_SHARDS: usize = 64;
const INITIAL_CAPACITY_PER_SHARD: usize = 1024;

/// Sharded mapping table from PID to page state pointer.
pub struct MappingTable {
    shards: Vec<RwLock<Shard>>,
    next_pid: AtomicU64,
}

struct Shard {
    entries: Vec<AtomicPtr<PageState>>,
}

impl MappingTable {
    /// Create a new empty mapping table.
    pub fn new() -> Self {
        let shards = (0..NUM_SHARDS)
            .map(|_| {
                RwLock::new(Shard {
                    entries: (0..INITIAL_CAPACITY_PER_SHARD)
                        .map(|_| AtomicPtr::new(ptr::null_mut()))
                        .collect(),
                })
            })
            .collect();
        Self {
            shards,
            next_pid: AtomicU64::new(1), // reserve NULL_PID
        }
    }

    /// Allocate a fresh PID.
    ///
    /// PIDs are allocated monotonically and never reused in the first version.
    pub fn allocate_pid(&self) -> Pid {
        self.next_pid.fetch_add(1, Ordering::SeqCst)
    }

    /// Return the next PID that would be allocated (for checkpointing).
    pub fn next_pid(&self) -> Pid {
        self.next_pid.load(Ordering::SeqCst)
    }

    /// Ensure the next allocated PID is at least `min_next_pid`.
    pub fn reserve_next_pid(&self, min_next_pid: Pid) {
        let mut current = self.next_pid.load(Ordering::Relaxed);
        while current < min_next_pid {
            match self.next_pid.compare_exchange_weak(
                current,
                min_next_pid,
                Ordering::SeqCst,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
    }

    /// Mark a PID as free.
    ///
    /// In the first version the PID is not reused; `u64` provides enough headroom
    /// that this is acceptable for testing and moderate workloads.
    pub fn free_pid(&self, _pid: Pid) {}

    /// Load the page state pointer for `pid`.
    pub fn load(&self, pid: Pid) -> Option<*mut PageState> {
        let (shard, idx) = self.locate(pid);
        let guard = shard.read();
        if idx >= guard.entries.len() {
            return None;
        }
        let ptr = guard.entries[idx].load(Ordering::Acquire);
        if ptr.is_null() {
            None
        } else {
            Some(ptr)
        }
    }

    /// Store a page state pointer for `pid`.
    pub fn store(&self, pid: Pid, ptr: *mut PageState) {
        let (shard, idx) = self.locate(pid);
        let guard = shard.read();
        if idx >= guard.entries.len() {
            drop(guard);
            self.grow(pid);
            return self.store(pid, ptr);
        }
        guard.entries[idx].store(ptr, Ordering::Release);
    }

    /// Compare-and-exchange the page state pointer for `pid`.
    pub fn compare_exchange(
        &self,
        pid: Pid,
        current: *mut PageState,
        new: *mut PageState,
    ) -> std::result::Result<*mut PageState, *mut PageState> {
        let (shard, idx) = self.locate(pid);
        let guard = shard.read();
        if idx >= guard.entries.len() {
            drop(guard);
            self.grow(pid);
            return self.compare_exchange(pid, current, new);
        }
        guard.entries[idx].compare_exchange(current, new, Ordering::AcqRel, Ordering::Acquire)
    }

    fn locate(&self, pid: Pid) -> (&RwLock<Shard>, usize) {
        let shard_index = pid as usize % NUM_SHARDS;
        let entry_index = pid as usize / NUM_SHARDS;
        (&self.shards[shard_index], entry_index)
    }

    fn grow(&self, pid: Pid) {
        let (shard, idx) = self.locate(pid);
        let mut guard = shard.write();
        let needed = idx + 1;
        let current_len = guard.entries.len();
        if needed > current_len {
            guard.entries.reserve(needed - current_len);
            while guard.entries.len() < needed {
                guard.entries.push(AtomicPtr::new(ptr::null_mut()));
            }
        }
    }
}

impl Default for MappingTable {
    fn default() -> Self {
        Self::new()
    }
}

/// Retire a chain of page states into the current epoch's garbage list. The
/// chain is reclaimed only after all pins from the current epoch have drained.
pub(crate) unsafe fn retire_chain(head: *mut PageState, guard: &epoch::Guard) {
    if head.is_null() {
        return;
    }
    // Retire the head; the rest of the chain is reachable through `next` and
    // will be dropped recursively when the head is reclaimed.
    unsafe {
        guard.defer_unchecked(move || {
            let _ = Box::from_raw(head);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page::{BaseNode, LeafBase, NodeHeader, Payload};

    fn dummy_state() -> *mut PageState {
        Box::into_raw(Box::new(PageState::new(
            NodeHeader::default(),
            Payload::Base(BaseNode::Leaf(LeafBase { entries: Vec::new() })),
            ptr::null_mut(),
            0,
        )))
    }

    #[test]
    fn allocate_monotonic() {
        let table = MappingTable::new();
        assert_eq!(table.allocate_pid(), 1);
        assert_eq!(table.allocate_pid(), 2);
        assert_eq!(table.allocate_pid(), 3);
    }

    #[test]
    fn store_and_load() {
        let table = MappingTable::new();
        let pid = table.allocate_pid();
        let state = dummy_state();
        table.store(pid, state);
        let loaded = table.load(pid).unwrap();
        assert_eq!(loaded, state);
        unsafe {
            let _ = Box::from_raw(state);
        }
    }

    #[test]
    fn cas_success_and_failure() {
        let table = MappingTable::new();
        let pid = table.allocate_pid();
        let a = dummy_state();
        let b = dummy_state();
        table.store(pid, a);
        assert!(table.compare_exchange(pid, a, b).is_ok());
        assert_eq!(table.load(pid).unwrap(), b);
        assert!(table.compare_exchange(pid, a, b).is_err());
        unsafe {
            let _ = Box::from_raw(a);
            let _ = Box::from_raw(b);
        }
    }
}
