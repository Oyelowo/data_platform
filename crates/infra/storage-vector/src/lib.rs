//! Durable vector / embedding storage engine.
//!
//! `storage-vector` provides an embeddable, crash-safe vector database with:
//!
//! * Multiple ANN indexes: brute force, HNSW, and IVF.
//! * Distance metrics: Euclidean (L2), cosine, dot product.
//! * Optional scalar quantization for memory reduction.
//! * WAL-backed durability and recovery.
//! * A `storage_traits::Engine` implementation for byte-key / vector-value access.
//!
//! # Example
//!
//! ```rust,no_run
//! use storage_vector::{VectorEngine, VectorOptions, DistanceMetric, IndexType};
//!
//! let dir = tempfile::tempdir().unwrap();
//! let opts = VectorOptions {
//!     dimension: 128,
//!     metric: DistanceMetric::Euclidean,
//!     index_type: IndexType::Hnsw,
//!     ..VectorOptions::default()
//! };
//! let engine = VectorEngine::open(dir.path(), opts).unwrap();
//! engine.put(b"doc-1", &vec![1.0f32; 128]).unwrap();
//! let results = engine.search(&vec![1.0f32; 128], 10).unwrap();
//! ```

#![warn(missing_docs)]
#![deny(clippy::unwrap_used)]
#![cfg_attr(test, allow(clippy::unwrap_used))]

pub mod cursor;
pub mod distance;
pub mod engine;
pub mod error;
pub mod format;
pub mod index;
pub mod options;
pub mod quantization;
pub mod recovery;
pub mod stats;
pub mod storage;
pub mod wal;

pub use cursor::VectorCursor;
pub use distance::DistanceMetric;
pub use engine::{VectorEngine, VectorTransaction};
pub use error::{Error, Result};
pub use index::{SearchResult, VectorIndex};
pub use options::{HnswOptions, IndexType, IvfOptions, Quantization, VectorOptions};
pub use quantization::ScalarQuantizer;
pub use stats::VectorStats;
