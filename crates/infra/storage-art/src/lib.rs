//! Adaptive Radix Trie (ART) in-memory ordered index.
//!
//! This crate provides a compressed prefix-tree index for byte keys and values.
//! It is designed as a low-level building block for higher-level storage engines
//! such as secondary indexes, graph adjacency indexes, and search term dictionaries.
//!
//! # Status
//!
//! The crate skeleton and design doc are in place; the implementation is deferred
//! until the existing persistent storage engines have been hardened.

#![warn(missing_docs)]

pub mod cursor;
pub mod error;
pub mod map;
pub mod node;
pub mod snapshot;

pub use cursor::ArtCursor;
pub use error::{Error, Result};
pub use map::{ArtMap, ArtMapOptions};
pub use node::{Node, NodeType};
