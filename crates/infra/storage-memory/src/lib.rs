//! High-performance in-memory storage engine.
//!
//! This crate provides [`MemoryEngine`], a [`storage_traits::Engine`]
//! implementation backed by `crossbeam-skiplist`. It is lock-free on the hot
//! path and serves as the first conformant engine in the storage layer.

#![warn(missing_docs)]
#![deny(clippy::unwrap_used)]
#![cfg_attr(test, allow(clippy::unwrap_used))]

pub mod cursor;
pub mod engine;
pub mod transaction;

pub use engine::{MemoryEngine, MAX_KEY_SIZE, MAX_VALUE_SIZE};
pub use transaction::MemoryTransaction;
