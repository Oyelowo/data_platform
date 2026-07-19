//! Error types for QIR lowering, planning, and execution.

use thiserror::Error;

/// Top-level result type for the QIR pipeline.
pub type QirResult<T> = Result<T, QirError>;

/// Top-level error enum for the QIR pipeline.
#[derive(Debug, Error)]
pub enum QirError {
    #[error("lowering error: {0}")]
    Lowering(#[from] LoweringError),
    #[error("planning error: {0}")]
    Planning(#[from] PlanError),
    #[error("execution error: {0}")]
    Execution(#[from] ExecError),
}

/// Errors produced while lowering HIR to logical QIR.
#[derive(Debug, Error)]
pub enum LoweringError {
    #[error("unsupported HIR expression in query context")]
    UnsupportedExpr,
    #[error("unsupported selector form")]
    UnsupportedSelector,
    #[error("unsupported query clause")]
    UnsupportedClause,
    #[error("range bound is not a literal and the backend does not support dynamic slicing")]
    NonLiteralRange,
    #[error("group by keys reference binders from multiple roots")]
    AmbiguousGroupTarget,
    #[error("edge type `{0}` is missing required endpoint fields")]
    InvalidEdgeEndpoints(String),
    #[error("correlated subquery could not be decorrelated")]
    Undecorrelatable,
    #[error("expected queryable source, got `{0}`")]
    NotQueryable(String),
    #[error("expected aggregate implementation for `{0}`")]
    MissingAggregate(String),
    #[error("slice requires an ordered (Seq) source")]
    SliceOnUnordered,
}

/// Errors produced while planning or executing a physical plan.
#[derive(Debug, Error)]
pub enum PlanError {
    #[error("required distribution cannot be satisfied")]
    UnsupportedDistribution,
    #[error("required ordering cannot be satisfied")]
    UnsupportedOrdering,
    #[error("aggregate `{0}` is not supported by backend")]
    UnsupportedAggregate(String),
    #[error("operator `{0}` is not supported by backend")]
    UnsupportedOperator(String),
    #[error("no valid physical plan found")]
    NoValidPlan,
    #[error("execution error: {0}")]
    Execution(String),
}

/// Errors produced during query execution.
#[derive(Debug, Error)]
pub enum ExecError {
    #[error("pipeline error: {0}")]
    Pipeline(String),
    #[error("spill error: {0}")]
    Spill(String),
    #[error("exchange error: {0}")]
    Exchange(String),
    #[error("kernel not found: {0}")]
    KernelNotFound(String),
    #[error("type mismatch in kernel")]
    TypeMismatch,
    #[error("out of memory")]
    OutOfMemory,
}
