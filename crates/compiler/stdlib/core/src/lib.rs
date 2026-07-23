//! Yelang standard library core module.
//!
//! Provides the foundational types and operations available to all Yelang
//! programs without explicit imports: memory management hooks and platform
//! abstractions.
//!
//! The `Primitive` marker trait and its primitive impls now live in the Yelang
//! prelude (`primitives.ye`) rather than in this Rust host module.

pub mod memory;
pub mod panic;
