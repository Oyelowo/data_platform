//! Optimistic-lock-coupling version latch.
//!
//! Each internal node owns a `VersionLatch`. Readers optimistically record the
//! version, read node data, then check that the version has not changed. If it
//! changed, the reader restarts. Writers spin-lock the latch (making the
//! version odd), mutate the node, then increment to the next even version.

use std::sync::atomic::{AtomicU64, Ordering};

/// A version latch for optimistic lock coupling.
#[derive(Debug)]
pub struct VersionLatch {
    version: AtomicU64,
}

impl Default for VersionLatch {
    fn default() -> Self {
        Self::new()
    }
}

impl VersionLatch {
    /// Create a new latch in the unlocked state (version 0).
    pub const fn new() -> Self {
        Self {
            version: AtomicU64::new(0),
        }
    }

    /// Try to begin a read-protected section. Returns the current version.
    ///
    /// The caller must later call [`read_unlock`] with the returned value and
    /// retry if it returns `false`.
    pub fn read_lock(&self) -> u64 {
        self.version.load(Ordering::Acquire)
    }

    /// Verify that the latch has not changed since `expected` was obtained.
    pub fn read_unlock(&self, expected: u64) -> bool {
        self.version.load(Ordering::Acquire) == expected
    }

    /// Returns true if the latch is currently write-locked.
    pub fn is_locked(&self) -> bool {
        self.version.load(Ordering::Acquire) % 2 == 1
    }

    /// Acquire the write lock. Spins until the latch is free.
    pub fn write_lock(&self) {
        loop {
            let v = self.version.load(Ordering::Relaxed);
            if v % 2 == 1 {
                std::hint::spin_loop();
                continue;
            }
            if self
                .version
                .compare_exchange_weak(v, v + 1, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                return;
            }
        }
    }

    /// Acquire the write lock and return a guard that releases it on drop.
    pub fn write_guard(&self) -> WriteGuard<'_> {
        self.write_lock();
        WriteGuard { latch: self }
    }

    /// Release the write lock and increment the version.
    ///
    /// # Safety
    ///
    /// The caller must hold the write lock.
    pub unsafe fn write_unlock(&self) {
        let v = self.version.load(Ordering::Relaxed);
        debug_assert_eq!(v % 2, 1, "write_unlock without write lock");
        self.version.store(v + 1, Ordering::Release);
    }
}

/// A scoped write lock guard for a [`VersionLatch`].
pub struct WriteGuard<'a> {
    latch: &'a VersionLatch,
}

impl<'a> Drop for WriteGuard<'a> {
    fn drop(&mut self) {
        unsafe { self.latch.write_unlock() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_latch_is_unlocked() {
        let latch = VersionLatch::new();
        assert!(!latch.is_locked());
        let v = latch.read_lock();
        assert!(latch.read_unlock(v));
    }

    #[test]
    fn write_lock_changes_version() {
        let latch = VersionLatch::new();
        let v0 = latch.read_lock();
        {
            let _guard = latch.write_guard();
            assert!(latch.is_locked());
            assert!(!latch.read_unlock(v0));
        }
        assert!(!latch.is_locked());
    }

    #[test]
    fn write_unlock_bumps_version() {
        let latch = VersionLatch::new();
        {
            let _guard = latch.write_guard();
        }
        let v = latch.read_lock();
        assert_eq!(v, 2);
    }

    #[test]
    fn guard_releases_on_panic() {
        let latch = VersionLatch::new();
        let result = std::panic::catch_unwind(|| {
            let _guard = latch.write_guard();
            panic!("intentional");
        });
        assert!(result.is_err());
        assert!(!latch.is_locked());
    }
}
