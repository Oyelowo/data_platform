//! Errors from sandbox/limit enforcement.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SandboxError {
    #[error("expansion exceeded memory limit")]
    MemoryLimitExceeded,
    #[error("expansion exceeded time limit")]
    TimeLimitExceeded,
    #[error("expansion produced too many tokens")]
    OutputLimitExceeded,
}
