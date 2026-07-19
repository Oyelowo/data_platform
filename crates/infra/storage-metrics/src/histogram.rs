//! Histogram metric.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// A simple histogram that tracks count, sum, min, and max.
///
/// This is intentionally simpler than HDRHistogram or a bucket-based histogram
/// to keep dependencies minimal. Future versions can add quantile sketches.
#[derive(Debug, Clone, Default)]
pub struct Histogram {
    inner: Arc<HistogramInner>,
}

impl Histogram {
    /// Create a new empty histogram.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a value.
    pub fn record(&self, value: u64) {
        self.inner.count.fetch_add(1, Ordering::Relaxed);
        self.inner.sum.fetch_add(value, Ordering::Relaxed);

        // Update min.
        let mut current = self.inner.min.load(Ordering::Relaxed);
        loop {
            if current != u64::MAX && current <= value {
                break;
            }
            match self
                .inner
                .min
                .compare_exchange(current, value, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => break,
                Err(v) => current = v,
            }
        }

        // Update max.
        let mut current = self.inner.max.load(Ordering::Relaxed);
        loop {
            if current >= value {
                break;
            }
            match self
                .inner
                .max
                .compare_exchange(current, value, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => break,
                Err(v) => current = v,
            }
        }
    }

    /// Return a snapshot of the histogram.
    pub fn snapshot(&self) -> HistogramSnapshot {
        let count = self.inner.count.load(Ordering::Relaxed);
        let sum = self.inner.sum.load(Ordering::Relaxed);
        let min = if count == 0 {
            0
        } else {
            self.inner.min.load(Ordering::Relaxed)
        };
        let max = self.inner.max.load(Ordering::Relaxed);
        HistogramSnapshot {
            count,
            sum,
            min,
            max,
        }
    }
}

#[derive(Debug)]
struct HistogramInner {
    count: AtomicU64,
    sum: AtomicU64,
    min: AtomicU64,
    max: AtomicU64,
}

impl HistogramInner {
    fn new() -> Self {
        Self {
            count: AtomicU64::new(0),
            sum: AtomicU64::new(0),
            min: AtomicU64::new(u64::MAX),
            max: AtomicU64::new(0),
        }
    }
}

impl Default for HistogramInner {
    fn default() -> Self {
        Self::new()
    }
}

/// A snapshot of histogram state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistogramSnapshot {
    /// Number of recorded values.
    pub count: u64,
    /// Sum of recorded values.
    pub sum: u64,
    /// Minimum recorded value (zero if empty).
    pub min: u64,
    /// Maximum recorded value.
    pub max: u64,
}
