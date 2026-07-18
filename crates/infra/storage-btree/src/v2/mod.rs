//! Next-generation in-place B+ tree building blocks.
//!
//! This module holds the slotted-page format, disk I/O primitives, and related
//! utilities for the redesigned `storage-btree` engine. It lives under `v2`
//! only during the transition; once the redesign reaches parity, the contents
//! will move to the crate root and the old COW modules will be removed.

#![allow(dead_code)]

pub mod buffer;
pub mod disk;
pub mod page;
pub mod slot;
pub mod space;
pub mod tree;
