//! Analytical columnar storage engine implementing `storage_traits::ColumnarEngine`.
//!
//! `storage-columnar` stores tables as collections of Apache Parquet files,
//! keeps an Arrow-compatible schema, and uses a `storage_wal`-backed manifest
//! for crash-safe durability.
//!
//! See `.doc/storage-columnar-design.md` for the full architecture and
//! checklist.

pub mod compaction;
pub mod engine;
pub mod error;
pub mod manifest;
pub mod options;
pub mod partition;
pub mod predicate;
pub mod reader;
pub mod schema;
pub mod snapshot;
pub mod types;
pub mod writer;

pub use engine::ColumnarEngineImpl;
pub use error::{Error, Result};
pub use manifest::{ColumnStats, FileMeta, Manifest};
pub use options::ColumnarOptions;
pub use schema::{ColumnDef, TableSchema};
pub use types::ColumnType;

pub(crate) mod manifest_wal;
pub(crate) mod pin;
