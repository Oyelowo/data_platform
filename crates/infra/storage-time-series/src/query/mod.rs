//! Query API for the time-series engine.

pub mod aggregate;
pub mod range;

pub use aggregate::{AggregateResult, Aggregation, aggregate_samples, aggregate_samples_binned};
pub use range::RangeCursor;

use crate::format::{Sample, Timestamp};

/// Tag filter used in queries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagFilter {
    /// Match series where `key == value`.
    Eq {
        /// Tag key.
        key: String,
        /// Tag value.
        value: String,
    },
    /// Match series where `key != value`.
    Neq {
        /// Tag key.
        key: String,
        /// Tag value.
        value: String,
    },
    /// Match series that have the given tag key.
    HasKey {
        /// Tag key.
        key: String,
    },
}

/// A time-series query.
#[derive(Debug, Clone, Default)]
pub struct Query {
    /// Metric name to match.
    pub metric: Vec<u8>,
    /// Tag filters.
    pub filters: Vec<TagFilter>,
    /// Half-open time range `[start, end)`.
    pub time_range: (Timestamp, Timestamp),
    /// Optional aggregation.
    pub aggregation: Option<Aggregation>,
}

impl Query {
    /// Create a new query for the given metric.
    pub fn new(metric: impl Into<Vec<u8>>) -> Self {
        Self {
            metric: metric.into(),
            ..Self::default()
        }
    }

    /// Restrict to a time range.
    pub fn range(mut self, start: Timestamp, end: Timestamp) -> Self {
        self.time_range = (start, end);
        self
    }

    /// Add an equality filter.
    pub fn eq(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.filters.push(TagFilter::Eq {
            key: key.into(),
            value: value.into(),
        });
        self
    }

    /// Add a not-equal filter.
    pub fn neq(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.filters.push(TagFilter::Neq {
            key: key.into(),
            value: value.into(),
        });
        self
    }

    /// Add a has-key filter.
    pub fn has_key(mut self, key: impl Into<String>) -> Self {
        self.filters.push(TagFilter::HasKey { key: key.into() });
        self
    }

    /// Set an aggregation.
    pub fn aggregate(mut self, agg: Aggregation) -> Self {
        self.aggregation = Some(agg);
        self
    }
}

/// Raw query result.
#[derive(Debug, Clone)]
pub struct QueryResult {
    /// Series key → ordered samples.
    pub series: std::collections::BTreeMap<Vec<u8>, Vec<Sample>>,
    /// Optional per-series aggregated scalars.
    pub aggregates: Option<std::collections::BTreeMap<Vec<u8>, AggregateResult>>,
}
