//! Gauge metric.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

/// An integer gauge that can go up and down.
#[derive(Debug, Clone, Default)]
pub struct Gauge {
    value: Arc<AtomicI64>,
}

impl Gauge {
    /// Create a new gauge initialized to zero.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the gauge to `value`.
    pub fn set(&self, value: i64) {
        self.value.store(value, Ordering::Relaxed);
    }

    /// Add `delta` to the gauge.
    pub fn add(&self, delta: i64) {
        self.value.fetch_add(delta, Ordering::Relaxed);
    }

    /// Subtract `delta` from the gauge.
    pub fn sub(&self, delta: i64) {
        self.value.fetch_sub(delta, Ordering::Relaxed);
    }

    /// Return the current value.
    pub fn get(&self) -> i64 {
        self.value.load(Ordering::Relaxed)
    }
}
