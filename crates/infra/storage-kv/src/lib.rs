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

mod backup;
mod blob;
mod blob_gc;
mod cache;
pub mod column_family;
mod compaction;
mod compaction_merge;
mod compaction_worker;
mod compression;
mod cursor;
mod engine;
mod error;
mod file;
mod file_number;
mod flush;
mod immutable;
pub mod internal_key;
pub mod logger;
mod manifest;
mod memtable;
pub mod merge_iter;
mod metrics;
mod obsolete_files;
mod options;
mod recovery;
mod sequence;
mod snapshots;
mod transaction;
mod txn_cursor;
mod version;
mod version_set;
mod wal;
mod worker;

pub mod sstable;

pub use backup::{
    BackupColumnFamily, BackupManifest, create_backup, create_checkpoint, delete_backup,
    list_backups, restore_backup, restore_checkpoint,
};
pub use engine::LsmEngine;
pub use error::{Error, Result};
pub use options::LsmOptions;

/// Sequence number ordering: newer writes have **larger** sequence numbers.
/// A snapshot with sequence `S` sees entries with sequence `<= S`.
pub type SequenceNumber = u64;

/// File number for SSTables and MANIFEST files.
pub type FileNumber = u64;
