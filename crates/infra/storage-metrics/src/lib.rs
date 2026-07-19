//! Shared metrics primitives for storage engines.
//!
//! This crate provides a lightweight, thread-safe metrics registry with
//! counters, gauges, and histograms. It is intentionally simple so that it
//! can be embedded in low-level engines without heavy dependencies.

#![warn(missing_docs)]
#![deny(clippy::unwrap_used)]

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;

pub mod counter;
pub mod gauge;
pub mod histogram;

pub use counter::Counter;
pub use gauge::Gauge;
pub use histogram::{Histogram, HistogramSnapshot};

/// A thread-safe metrics registry.
#[derive(Debug, Clone, Default)]
pub struct Registry {
    inner: Arc<Mutex<HashMap<String, Metric>>>,
}

impl Registry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Get or create a counter named `name`.
    pub fn counter(&self, name: impl Into<String>) -> Counter {
        let name = name.into();
        let mut inner = self.inner.lock();
        match inner.entry(name.clone()) {
            std::collections::hash_map::Entry::Occupied(e) => {
                if let Metric::Counter(c) = e.get() {
                    c.clone()
                } else {
                    panic!("metric {name} exists but is not a counter");
                }
            }
            std::collections::hash_map::Entry::Vacant(e) => {
                let c = Counter::new();
                e.insert(Metric::Counter(c.clone()));
                c
            }
        }
    }

    /// Get or create a gauge named `name`.
    pub fn gauge(&self, name: impl Into<String>) -> Gauge {
        let name = name.into();
        let mut inner = self.inner.lock();
        match inner.entry(name.clone()) {
            std::collections::hash_map::Entry::Occupied(e) => {
                if let Metric::Gauge(g) = e.get() {
                    g.clone()
                } else {
                    panic!("metric {name} exists but is not a gauge");
                }
            }
            std::collections::hash_map::Entry::Vacant(e) => {
                let g = Gauge::new();
                e.insert(Metric::Gauge(g.clone()));
                g
            }
        }
    }

    /// Get or create a histogram named `name`.
    pub fn histogram(&self, name: impl Into<String>) -> Histogram {
        let name = name.into();
        let mut inner = self.inner.lock();
        match inner.entry(name.clone()) {
            std::collections::hash_map::Entry::Occupied(e) => {
                if let Metric::Histogram(h) = e.get() {
                    h.clone()
                } else {
                    panic!("metric {name} exists but is not a histogram");
                }
            }
            std::collections::hash_map::Entry::Vacant(e) => {
                let h = Histogram::new();
                e.insert(Metric::Histogram(h.clone()));
                h
            }
        }
    }

    /// Return a snapshot of all metrics.
    pub fn snapshot(&self) -> HashMap<String, MetricValue> {
        self.inner
            .lock()
            .iter()
            .map(|(name, metric)| (name.clone(), metric.snapshot()))
            .collect()
    }
}

#[derive(Debug, Clone)]
enum Metric {
    Counter(Counter),
    Gauge(Gauge),
    Histogram(Histogram),
}

impl Metric {
    fn snapshot(&self) -> MetricValue {
        match self {
            Metric::Counter(c) => MetricValue::Counter(c.get()),
            Metric::Gauge(g) => MetricValue::Gauge(g.get()),
            Metric::Histogram(h) => MetricValue::Histogram(h.snapshot()),
        }
    }
}

/// A snapshot value of a metric.
#[derive(Debug, Clone, PartialEq)]
pub enum MetricValue {
    /// Counter value.
    Counter(u64),
    /// Gauge value.
    Gauge(i64),
    /// Histogram snapshot (count, sum, min, max, buckets).
    Histogram(HistogramSnapshot),
}
