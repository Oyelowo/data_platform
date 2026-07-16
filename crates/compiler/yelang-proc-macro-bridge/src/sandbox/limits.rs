//! Resource limits for proc-macro execution.

use serde::{Deserialize, Serialize};

/// Per-expansion limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    /// Maximum heap size in bytes.
    pub max_heap_bytes: usize,
    /// Maximum CPU time in seconds.
    pub max_cpu_seconds: u64,
    /// Maximum number of output tokens.
    pub max_output_tokens: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_heap_bytes: 512 * 1024 * 1024, // 512 MiB
            max_cpu_seconds: 30,
            max_output_tokens: 1_000_000,
        }
    }
}
