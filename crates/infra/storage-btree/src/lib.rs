//! Persistent copy-on-write B+ tree key-value storage engine.
//!
//! `storage-btree` provides a durable, thread-safe B+ tree implementation of the
//! [`storage_traits::Engine`] contract. It is intended as a read-heavy /
//! range-scan alternative to the LSM engine in `storage-kv`.
//!
//! # Design overview
//!
//! The engine uses a **copy-on-write (COW)** B+ tree:
//!
//! * Pages on disk are immutable; they are never overwritten in place.
//! * Every modifying operation clones the path from root to leaf, writes new
//!   pages, and installs a new root atomically.
//! * Readers traverse immutable pages without locks, using the root pointer
//!   captured at the start of their operation.
//! * Writes are serialized by a single writer lock, but the locked section is
//!   short because the heavy work (page I/O) happens on immutable pages.
//! * Logical operations are appended to a `storage-wal` before the in-memory
//!   tree is updated, enabling crash recovery.

#![warn(missing_docs)]
#![deny(clippy::unwrap_used)]
#![cfg_attr(test, allow(clippy::unwrap_used))]

mod cursor;
mod engine;
mod error;
mod node;
mod options;
mod page;
mod pager;
mod recovery;
mod transaction;
mod tree;
mod wal_record;

pub use cursor::BtreeCursor;
pub use engine::BtreeEngine;
pub use error::{Error, Result};
pub use options::BtreeOptions;
pub use transaction::BtreeTransaction;
