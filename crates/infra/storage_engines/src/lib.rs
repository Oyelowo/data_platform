//! Storage engine abstractions for the data platform.
//!
//! This crate re-exports the storage trait API and the engine family so that
//! callers can depend on a single crate while the engine family grows.

#![warn(missing_docs)]

pub use storage_blob::{BlobStoreImpl, BlobStoreOptions};
pub use storage_btree::{BtreeEngine, BtreeOptions};
pub use storage_bwtree::{BwTreeEngine, BwTreeOptions};
pub use storage_columnar::{ColumnarEngineImpl, ColumnarOptions};
pub use storage_memory::{MemoryEngine, MemoryTransaction};
pub use storage_traits::*;
