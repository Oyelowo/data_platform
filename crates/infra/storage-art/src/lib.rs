//! Adaptive Radix Trie (ART) storage engine.
//!
//! This crate provides both an in-memory ordered index ([`ArtMap`]) and a
//! durable engine ([`ArtEngine`]) that persists the tree to disk using a
//! snapshot file and a write-ahead log.
//!
//! # Concurrency
//!
//! The [`ArtMap`] implementation uses Optimistic Lock Coupling (OLC): readers
//! traverse the tree without locking, restarting whenever a node's version
//! changes, while writers lock-couple down the tree holding the parent latch
//! while installing child pointers.
//!
//! The durable [`ArtEngine`] keeps those lock-free reads and serializes durable
//! writers through a single engine-level write lock. This gives one durable
//! writer and many concurrent readers.

#![warn(missing_docs)]

pub mod cursor;
pub mod durable;
pub mod engine;
pub mod error;
pub mod format;
pub mod keys;
pub(crate) mod latch;
pub mod map;
pub mod node;
pub mod nodes;
pub mod options;
pub mod recovery;
pub mod snapshot;
pub mod stats;

pub use cursor::ArtCursor;
pub use durable::{ArtEngine, ArtEngineTransaction};
pub use engine::ArtTransaction;
pub use error::{Error, Result};
pub use map::ArtMap;
pub use node::{Node, NodeType};
pub use options::{ArtEngineOptions, ArtMapOptions, WalSyncPolicy};
