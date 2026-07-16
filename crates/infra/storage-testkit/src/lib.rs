//! Conformance, property-based, and workload tests for storage engines.
//!
//! This crate provides a shared specification that every engine must satisfy.
//! Engine crates call [`conformance::run`] and [`property::run`] from their own
//! test suites.

#![warn(missing_docs)]
// Test helpers intentionally use unwrap/assert for failed-engine diagnostics.
#![allow(clippy::unwrap_used)]

pub mod conformance;
pub mod fault;
pub mod model;
pub mod property;
pub mod workload;

pub use model::{Model, Op};
