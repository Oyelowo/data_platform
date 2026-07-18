//! Optimistic Lock Coupling (OLC) primitives.
//!
//! The latch word is a `u64` with bit 0 as the exclusive-lock bit and bits
//! 1..63 as a monotonic version counter.  Readers snapshot the version, read
//! the page, and check that the version is unchanged and the lock bit is not
//! set.  Writers atomically set the lock bit, modify the page, and release by
//! incrementing the version and clearing the lock bit.
//!
//! All operations are wait-free except the spin in `try_lock`, which is bounded
//! by the writer's critical section and is a standard OLC pattern.

use std::sync::atomic::{AtomicU64, Ordering};

const LOCK_BIT: u64 = 0x0000_0000_0000_0001;
const VERSION_MASK: u64 = !LOCK_BIT;

/// A versioned OLC latch.
#[derive(Debug, Default)]
pub struct Latch {
    word: AtomicU64,
}

impl Latch {
    /// Create a new latch with version 0 and no lock.
    pub fn new() -> Self {
        Self {
            word: AtomicU64::new(0),
        }
    }

    /// Read the raw latch word.
    #[inline]
    pub fn word(&self) -> u64 {
        self.word.load(Ordering::Acquire)
    }

    /// True if the lock bit is currently set.
    #[inline]
    pub fn is_locked(&self) -> bool {
        self.word() & LOCK_BIT != 0
    }

    /// Return the current version if the latch is not locked.
    #[inline]
    pub fn optimistic_version(&self) -> Option<u64> {
        let word = self.word();
        if word & LOCK_BIT != 0 {
            None
        } else {
            Some(word)
        }
    }

    /// Try to acquire the exclusive lock. Returns the current version on
    /// success so the caller can release with `unlock`.
    pub fn try_lock(&self) -> Option<u64> {
        let mut current = self.word.load(Ordering::Relaxed);
        loop {
            if current & LOCK_BIT != 0 {
                return None;
            }
            match self.word.compare_exchange_weak(
                current,
                current | LOCK_BIT,
                Ordering::Acquire,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Some(current),
                Err(actual) => current = actual,
            }
        }
    }

    /// Release the exclusive lock and bump the version.
    ///
    /// # Safety
    ///
    /// The caller must hold the exclusive lock returned by `try_lock`.
    #[inline]
    pub unsafe fn unlock(&self) {
        // We add 2 to clear the lock bit and increment the version by one
        // (because the lock bit occupies bit 0).
        self.word.fetch_add(2, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_latch_is_unlocked_version_zero() {
        let latch = Latch::new();
        assert!(!latch.is_locked());
        assert_eq!(latch.optimistic_version(), Some(0));
    }

    #[test]
    fn lock_unlock_bumps_version() {
        let latch = Latch::new();
        let v0 = latch.try_lock().unwrap();
        assert!(latch.is_locked());
        assert_eq!(latch.optimistic_version(), None);
        unsafe { latch.unlock() };
        assert!(!latch.is_locked());
        let v1 = latch.optimistic_version().unwrap();
        assert_eq!(v1, v0 + 2);
    }

    #[test]
    fn second_lock_fails_while_locked() {
        let latch = Latch::new();
        let _v = latch.try_lock().unwrap();
        assert!(latch.try_lock().is_none());
        unsafe { latch.unlock() };
        assert!(latch.try_lock().is_some());
    }
}
