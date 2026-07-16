//! Fault-injection utilities for storage testing.
//!
//! This module will eventually provide a fault-injectable filesystem layer. For
//! Phase 0 it only exposes placeholder types.

/// Placeholder fault-injection filesystem.
#[derive(Debug, Default)]
pub struct FaultyFs;

impl FaultyFs {
    /// Create a new fault-injection filesystem.
    pub fn new() -> Self {
        Self
    }
}
