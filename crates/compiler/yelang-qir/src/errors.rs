//! Error types for QIR lowering, planning, and execution.

use thiserror::Error;

/// Result type used throughout `yelang-qir`.
pub type QirResult<T> = Result<T, QirError>;

/// Top-level error enum for the QIR pipeline.
#[derive(Debug, Error)]
pub enum QirError {
    #[error("lowering error: {0}")]
    Lowering(#[from] LoweringError),
    #[error("planning error: {0}")]
    Planning(#[from] PlanError),
}

/// Errors produced while lowering HIR to logical QIR.
#[derive(Debug, Error)]
pub enum LoweringError {
    #[error("unsupported selector form")]
    UnsupportedSelector,
    #[error("range bound is not a literal and the backend does not support dynamic slicing")]
    NonLiteralRange,
    #[error("group by keys reference binders from multiple roots")]
    AmbiguousGroupTarget,
    #[error("edge type `{0}` is missing required endpoint fields `_from` and `_to`")]
    InvalidEdgeEndpoints(String),
}

/// Errors produced while planning or executing a physical plan.
#[derive(Debug, Error)]
pub enum PlanError {
    #[error("required distribution cannot be satisfied")]
    UnsupportedDistribution,
    #[error("execution error: {0}")]
    Execution(String),
}
