//! Durable time-series storage engine.
//!
//! `storage-time-series` provides an embeddable, crash-safe time-series database
//! with:
//!
//! * Gorilla XOR compression for `f64` values and delta-of-delta timestamp
//!   compression.
//! * Optional Zstd compression for byte payloads.
//! * WAL-backed durability and recovery.
//! * Label-based inverted index for tag-filtered series discovery.
//! * In-engine aggregations: `Sum`, `Count`, `Avg`, `Min`, `Max`, `Rate`.
//! * TTL retention and simple chunk compaction.
//! * A `storage_traits::Engine` implementation for byte-key / encoded-value
//!   access.
//!
//! # Example
//!
//! ```rust,no_run
//! use storage_time_series::{TimeSeriesEngine, TimeSeriesOptions, Value};
//!
//! let dir = tempfile::tempdir().unwrap();
//! let engine = TimeSeriesEngine::open(dir.path(), TimeSeriesOptions::default()).unwrap();
//! engine.put(b"cpu\0host=db1".to_vec(), 1, Value::F64(0.5)).unwrap();
//! let latest = engine.get_latest(b"cpu\0host=db1").unwrap();
//! ```

#![warn(missing_docs)]
#![deny(clippy::unwrap_used)]
#![cfg_attr(test, allow(clippy::unwrap_used))]

pub mod chunk;
pub mod compaction;
pub mod cursor;
pub mod engine;
pub mod error;
pub mod format;
pub mod index;
pub mod memtable;
pub mod options;
pub mod query;
pub mod recovery;
pub mod stats;
pub mod transaction;
pub mod wal;

pub use engine::TimeSeriesEngine;
pub use error::{Error, Result};
pub use format::{Sample, Timestamp, Value, build_series_key, parse_series_key};
pub use options::{CompressionKind, RetentionPolicy, TimeSeriesOptions, ValueKind, WalSyncPolicy};
pub use query::{Query, QueryResult, TagFilter};
pub use stats::TimeSeriesStats;
pub use transaction::TimeSeriesTransaction;
