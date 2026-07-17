//! Public trait API for the data-platform storage layer.
//!
//! This crate defines the contracts that every storage engine must implement.
//! Callers depend on these traits rather than on concrete engines, allowing
//! engines to be swapped without changing higher-level code.
//!
//! # Design notes
//!
//! * The API is **byte-oriented**: keys and values are opaque byte sequences.
//! * The API is **synchronous**: engines are `Send + Sync` and can be used from
//!   any context. Internally, engines may use async runtimes or thread pools,
//!   but that is an implementation detail.
//! * The API is **minimal**: higher-level features (documents, graphs,
//!   columnar projections) are built on top of these primitives.

#![warn(missing_docs)]
#![deny(clippy::unwrap_used)]

pub mod blob;
pub mod columnar;
pub mod cursor;
pub mod engine;
pub mod error;
pub mod indexed;
pub mod options;
pub mod stats;
pub mod transaction;

pub use blob::BlobStore;
pub use columnar::{ColumnBatch, ColumnarEngine, Predicate, ScanResult};
#[cfg(feature = "async")]
pub use cursor::AsyncCursor;
pub use cursor::Cursor;
pub use engine::Engine;
pub use error::{BoundKind, Error, Result};
pub use indexed::IndexedEngine;
pub use options::{IsolationLevel, TxnOptions};
pub use stats::EngineStats;
pub use transaction::Transaction;
