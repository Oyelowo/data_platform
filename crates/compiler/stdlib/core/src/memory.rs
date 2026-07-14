//! Memory management abstractions for Yelang.
//!
//! Yelang does not have a borrow checker, but it still needs allocation,
//! deallocation, and optional garbage-collection hooks.

/// Runtime allocation trait. All heap-allocated types implement this.
pub trait Allocated {
    /// Requested alignment in bytes.
    fn alignment(&self) -> usize;
    /// Size in bytes.
    fn size(&self) -> usize;
}
