//! Shared durable file-system primitives for storage engines.
//!
//! This crate provides cross-platform helpers for atomic file writes,
//! directory fsync, and positional reads. These operations appear in every
//! persistent storage engine, so centralizing them avoids duplication and
//! ensures consistent durability semantics.

#![warn(missing_docs)]
#![deny(clippy::unwrap_used)]

pub mod atomic;
pub mod read;
pub mod sync;

pub use atomic::{atomic_write, atomic_write_with_permissions};
pub use read::{read_exact_at, write_all_at};
pub use sync::{open_dir_for_sync, sync_dir, sync_file};
