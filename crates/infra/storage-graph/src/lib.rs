//! Durable labeled property graph storage engine.
//!
//! `storage-graph` provides an embeddable, crash-safe property graph database
//! with:
//!
//! * Labeled nodes with arbitrary key-value properties.
//! * Directed edges with a single label and arbitrary properties.
//! * Dense internal IDs mapped from user byte-string IDs.
//! * Outgoing, incoming, and bidirectional adjacency indexes.
//! * Label indexes for fast node and edge scans.
//! * BFS/DFS traversal with optional edge-label filtering.
//! * Simple chain pattern matching.
//! * WAL-backed durability and recovery.
//! * Store compaction to reclaim space from deleted records.
//! * A `storage_traits::Engine` implementation for byte-key access.
//!
//! # Example
//!
//! ```rust,no_run
//! use storage_graph::{GraphEngine, GraphOptions, PropertyMap};
//!
//! let dir = tempfile::tempdir().unwrap();
//! let engine = GraphEngine::open(dir.path(), GraphOptions::default()).unwrap();
//! engine.create_node(b"alice", ["User"], PropertyMap::new()).unwrap();
//! engine.create_node(b"bob", ["User"], PropertyMap::new()).unwrap();
//! engine.create_edge(b"e1", b"alice", b"bob", "FOLLOWS", PropertyMap::new()).unwrap();
//! let neighbors = engine.neighbors(b"alice", storage_graph::Direction::Out, None).unwrap();
//! ```

#![warn(missing_docs)]
#![deny(clippy::unwrap_used)]
#![cfg_attr(test, allow(clippy::unwrap_used))]

pub mod compaction;
pub mod cursor;
pub mod engine;
pub mod error;
pub mod format;
pub mod id;
pub mod index;
pub mod model;
pub mod options;
pub mod query;
pub mod recovery;
pub mod stats;
pub mod store;
pub mod transaction;
pub mod wal;

pub use cursor::GraphCursor;
pub use engine::GraphEngine;
pub use error::{Error, Result};
pub use id::{InternalEdgeId, InternalNodeId};
pub use model::{Edge, LabelSet, Node, PropertyMap};
pub use options::{GraphOptions, WalSyncPolicy};
pub use query::pattern::PatternStep;
pub use query::{Direction, GraphQuery, QueryResult};
pub use stats::GraphStats;
pub use transaction::GraphTransaction;
