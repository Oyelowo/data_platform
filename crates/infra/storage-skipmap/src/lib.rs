//! Owned lock-free skip-map with epoch-based memory reclamation.
//!
//! This crate provides [`SkipMap`], an ordered concurrent map backed by a
//! lock-free skip list. It is intended for use as the MemTable in the LSM-tree
//! engine and as the backing store for the in-memory engine.

#![warn(missing_docs)]
#![deny(clippy::unwrap_used)]
#![cfg_attr(test, allow(clippy::unwrap_used))]

pub mod map;
pub mod node;

#[cfg(test)]
mod tests;

pub use map::{Cursor, SkipMap};
