//! Latch-free Bw-Tree key-value storage engine.
//!
//! `storage-bwtree` implements the [`storage_traits::Engine`] contract using a
//! delta-chain B+ tree. Reads are lock-free: threads pin an epoch, traverse the
//! mapping table and delta chains, and unpin. Writes install state by CAS-ing a
//! new delta record onto the chain head in the mapping table.
//!
//! # Known limitations
//!
//! * Structural modifications are serialized by a global SMO lock rather than
//!   the full ∆abort protocol from the literature. This is simpler and correct
//!   but limits concurrency under split/merge-heavy workloads.
//! * The WAL is never truncated in the first version because there is no
//!   durable page checkpoint. Recovery replays the full WAL from the start.
//!   This keeps recovery correct but makes open time grow with the write
//!   history until checkpointing is added.
//! * Leaf merges are disabled in the first version. Deleted entries leave
//!   underfull leaves; the tree remains correct but space is not reclaimed
//!   through merging.
//! * Overflow values are stored in a separate append-only file rather than a
//!   log-structured store.
//! * Only `ReadCommitted` transaction isolation is supported.
//! * Non-unique keys are not supported.

#![warn(missing_docs)]
#![deny(clippy::unwrap_used)]
#![cfg_attr(test, allow(clippy::unwrap_used))]

mod cursor;
mod engine;
mod error;
mod mapping_table;
mod node;
mod options;
mod overflow;
mod page;
mod recovery;
mod transaction;
mod tree;
mod wal_record;

pub use cursor::BwTreeCursor;
pub use engine::BwTreeEngine;
pub use error::{BoundKind, Error, Result};
pub use options::BwTreeOptions;
pub use transaction::BwTreeTransaction;
