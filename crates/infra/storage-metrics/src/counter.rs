//! Counter metric.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// A monotonically increasing counter.
#[derive(Debug, Clone, Default)]
pub struct Counter {
    value: Arc<AtomicU64>,
}

impl Counter {
    /// Create a new counter initialized to zero.
    pub fn new() -> Self {
        Self::default()
    }

    /// Increment the counter by one.
    pub fn inc(&self) {
        self.add(1);
    }

    /// Increment the counter by `n`.
    pub fn add(&self, n: u64) {
        self.value.fetch_add(n, Ordering::Relaxed);
    }

    /// Return the current value.
    pub fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }
}
