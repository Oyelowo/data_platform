//! Persistent in-place B+ tree key-value storage engine.
//!
//! `storage-btree` provides a durable, thread-safe B+ tree implementation of the
//! [`storage_traits::Engine`] contract. It is intended as a read-heavy /
//! range-scan alternative to the LSM engine in `storage-kv`.
//!
//! # Design overview
//!
//! The engine uses an **in-place B+ tree** with optimistic lock coupling
//! (OLC), physiological WAL (ARIES-style recovery), MVCC snapshot isolation,
//! a slotted page format, a buffer pool with background eviction, and a
//! separate value log for out-of-line large values:
//!
//! * Pages are mutable in place and protected by per-page optimistic latches.
//! * Readers traverse root-to-leaf optimistically, retrying if a page changes
//!   mid-read, so reads scale with concurrency and require no writer locks.
//! * Writers use latch crabbing with a fixed page-id ordering for
//!   structure-modifying operations (SMOs) to avoid deadlock.
//! * Logical operations are appended to a physiological WAL before in-memory
//!   pages are updated, enabling ARIES-style redo/undo crash recovery.
//! * Multi-version concurrency control (MVCC) provides snapshot isolation:
//!   each transaction reads a consistent snapshot while writers append new
//!   versions without blocking readers.
//! * Large values are stored in a dedicated value log; the B+ tree cells hold
//!   references, reducing tree fan-out pressure and simplifying compaction.
//! * A background page cleaner flushes dirty frames so foreground writes can
//!   amortise I/O.

#![warn(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(clippy::unwrap_used)]
#![cfg_attr(test, allow(clippy::unwrap_used))]

//! # Unsafe-code policy
//!
//! The in-place OLC B+ tree uses a small, audited amount of `unsafe` code for
//! the page buffer and latch word. The crate denies `unsafe_op_in_unsafe_fn`
//! and maintains an `UNSAFE_MANIFEST.md` listing every unsafe block, its
//! invariant, and why it is unavoidable. A fully safe implementation would
//! require either a global page mutex or a full page copy on every write,
//! both of which contradict the performance/scalability goals of the engine.

mod buffer;
mod checkpoint;
mod cleaner;
mod cursor;
mod disk;
mod engine;
mod error;
mod options;
mod page;
mod recovery;
mod slot;
mod space;
mod sync;
mod transaction;
mod tree;
mod txn;
mod undo;
mod valuelog;
mod version;
mod wal;

pub use cursor::BPlusTreeCursor as BtreeCursor;
pub use engine::BtreeEngine;
pub use error::{Error, Result};
pub use options::BtreeOptions;
pub use transaction::BtreeTransaction;
