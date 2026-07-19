//! Shared probabilistic filters for storage engines.
//!
//! This crate provides Bloom filters and related data structures that are used
//! to skip unnecessary disk reads in LSM-trees, vector indexes, and search
//! posting lists.

#![warn(missing_docs)]
#![deny(clippy::unwrap_used)]

pub mod bloom;
pub mod blocked;
pub mod cuckoo;

pub use bloom::{BloomFilterBuilder, BloomFilterReader};
pub use blocked::BlockedBloomFilter;
pub use cuckoo::CuckooFilter;
