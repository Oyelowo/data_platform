//! Persistent LSM-tree key-value storage engine.
//!
//! `storage-kv` provides a production-oriented embedded key-value store built on:
//!
//! * `storage-skipmap` for the in-memory MemTable.
//! * `storage-wal` for durable write-ahead logging.
//! * Block-based SSTables with Bloom filters and leveled compaction.
//! * A `MANIFEST` log for atomic metadata changes.
//!
//! The public API is synchronous and runtime-agnostic, matching the
//! `storage-traits` contract.

mod cache;
mod compaction;
mod cursor;
mod engine;
mod error;
mod flush;
mod immutable;
mod internal_key;
mod manifest;
mod memtable;
mod merge_iter;
mod options;
mod recovery;
mod transaction;
mod version;
mod version_set;
mod wal;
mod worker;

pub mod sstable;

pub use engine::LsmEngine;
pub use error::{Error, Result};
pub use options::LsmOptions;

/// Sequence number ordering: newer writes have **larger** sequence numbers.
/// A snapshot with sequence `S` sees entries with sequence `<= S`.
pub type SequenceNumber = u64;

/// File number for SSTables and MANIFEST files.
pub type FileNumber = u64;
