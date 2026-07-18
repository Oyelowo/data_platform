//! Deadlock-preventing lock wrappers for higher-level engine resources.
//!
//! The page-level optimistic latch in `page.rs` is a purpose-built atomic
//! versioned lock and stays on the hot path. Everything else (engine metadata,
//! transaction tables, retired-page lists, value-log bookkeeping) should use
//! these wrappers instead of raw `std::sync::Mutex`.
//!
//! The `Mutex` wrapper exposes only a closure-based accessor, so locks are
//! always released when the closure returns and the critical section is visible
//! to the reader. This eliminates forgotten unlocks and makes it impossible to
//! hold a lock across an `await` or a long-running computation by accident.
//!
//! When multiple mutexes must be held together, acquire them in a deterministic
//! order (e.g. by memory address) or inside nested `with_mut` calls that follow
//! a documented global order. Such helpers can be added here if the engine ever
//! needs them; the B+ tree currently holds at most one higher-level mutex at a
//! time.

use std::fmt::{self, Debug};

use parking_lot::Mutex as ParkingMutex;

/// A mutex that only exposes scoped, closure-based access.
///
/// The lock is held only for the duration of the closure, which prevents
/// forgotten unlocks and encourages small critical sections.
pub struct Mutex<T> {
    inner: ParkingMutex<T>,
}

impl<T> Mutex<T> {
    /// Create a new scoped mutex.
    pub const fn new(value: T) -> Self {
        Self {
            inner: ParkingMutex::new(value),
        }
    }

    /// Acquire the lock and call `f` with exclusive access to the protected
    /// value. The lock is released when `f` returns.
    pub fn with_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut T) -> R,
    {
        let mut guard = self.inner.lock();
        f(&mut *guard)
    }
}

impl<T: Default> Default for Mutex<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T: Debug> Debug for Mutex<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.with_mut(|v| f.debug_struct("Mutex").field("value", v).finish())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutex_with_mut_holds_lock_only_inside_closure() {
        let m = Mutex::new(0);
        m.with_mut(|v| *v = 7);
        m.with_mut(|v| assert_eq!(*v, 7));
    }
}
