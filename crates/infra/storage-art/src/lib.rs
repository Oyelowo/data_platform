//! Adaptive Radix Trie (ART) in-memory ordered index.
//!
//! This crate provides a compressed prefix-tree index for byte keys and values.
//! It is designed as a low-level building block for higher-level storage engines
//! such as secondary indexes, graph adjacency indexes, and search term dictionaries.
//!
//! # Concurrency
//!
//! The [`ArtMap`] implementation uses Optimistic Lock Coupling (OLC): readers
//! traverse the tree without locking, restarting whenever a node's version
//! changes, while writers lock-couple down the tree holding the parent latch
//! while installing child pointers.

#![warn(missing_docs)]

pub mod cursor;
pub mod engine;
pub mod error;
pub mod keys;
pub(crate) mod latch;
pub mod map;
pub mod node;
pub mod nodes;
pub mod options;
pub mod snapshot;
pub mod stats;

pub use cursor::ArtCursor;
pub use engine::ArtTransaction;
pub use error::{Error, Result};
pub use map::ArtMap;
pub use node::{Node, NodeType};
pub use options::ArtMapOptions;
