//! Workload generators for benchmarking and stress testing.
//!
//! Phase 0 provides only the module structure. Generators are added as engines
//! mature.

/// Placeholder workload generator.
#[derive(Debug, Default)]
pub struct Workload;

impl Workload {
    /// Create a new workload generator.
    pub fn new() -> Self {
        Self
    }
}
