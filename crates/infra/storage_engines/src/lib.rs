//! Storage engine abstractions for the data platform.
//!
//! This crate re-exports the storage trait API and the in-memory engine so that
//! callers can depend on a single crate while the engine family grows.

#![warn(missing_docs)]

pub use storage_memory::{MemoryEngine, MemoryTransaction};
pub use storage_traits::*;
