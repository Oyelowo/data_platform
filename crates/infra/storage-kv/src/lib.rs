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

mod compaction;
mod cursor;
mod engine;
mod error;
mod immutable;
mod internal_key;
mod manifest;
mod memtable;
mod options;
mod recovery;
mod transaction;
mod version;
mod version_set;
mod wal;

pub mod sstable;

pub use engine::LsmEngine;
pub use error::{Error, Result};
pub use options::LsmOptions;

/// Sequence number ordering: newer writes have **smaller** sequence numbers,
/// matching the RocksDB convention and making seek-by-snapshot natural.
pub type SequenceNumber = u64;

/// File number for SSTables and MANIFEST files.
pub type FileNumber = u64;
