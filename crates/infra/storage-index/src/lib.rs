//! Durable secondary-index storage engine.
//!
//! `storage-index` implements [`storage_traits::IndexedEngine`] on top of any
//! ordered engine. It stores primary records and secondary index entries in the
//! underlying engine, using the underlying engine's transactions to keep the
//! two consistent.
//!
//! Values written through this engine are [`Record`] values (a map of column
//! name → bytes). Values written by other callers are stored as opaque primary
//! records and do not participate in secondary indexes.

#![warn(missing_docs)]

pub mod catalog;
pub mod cursor;
pub mod engine;
pub mod error;
pub mod keys;
pub mod ops;
pub mod record;

pub use catalog::{IndexCatalog, IndexDef, IndexId, IndexState};
pub use engine::IndexEngine;
pub use error::{Error, Result};
pub use record::Record;
