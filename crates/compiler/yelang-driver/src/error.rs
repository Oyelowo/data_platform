//! Driver-level errors that can occur during end-to-end compilation or execution.

use yelang_hir::ids::{BodyId, QueryId};
use yelang_qir::errors::{LoweringError, PlanError, QirError};
use yelang_resolve::ResolutionError;
use yelang_tycheck::diagnostics::Diagnostic;

/// The result type returned by driver operations.
pub type Result<T> = std::result::Result<T, DriverError>;

/// Errors produced by the Yelang compiler driver.
#[derive(Debug, thiserror::Error)]
pub enum DriverError {
    /// Failed to read the standard library or user source from disk.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// Lexer or parser failure.
    #[error("parse error: {0}")]
    Parse(String),

    /// Name resolution produced errors.
    #[error("resolution errors: {0:?}")]
    Resolution(Vec<ResolutionError>),

    /// Type checking produced diagnostics.
    #[error("type errors: {0:?}")]
    TypeCheck(Vec<Diagnostic>),

    /// Lowering to HIR failed.
    #[error("hir lowering error: {0}")]
    HirLowering(String),

    /// No `main` function was found in the compiled crate.
    #[error("no `main` function found")]
    MissingMain,

    /// The `main` function has no body.
    #[error("`main` function has no body")]
    MainHasNoBody,

    /// No query expression was found inside `main`.
    #[error("no query found in `main` body")]
    MissingQuery,

    /// Lowering the query to QIR failed.
    #[error("qir lowering error: {0}")]
    QirLowering(#[from] LoweringError),

    /// Physical planning failed.
    #[error("physical planning error: {0}")]
    Planning(#[from] PlanError),

    /// A QIR-level error (lowering, planning, or execution).
    #[error("qir error: {0}")]
    Qir(#[from] QirError),

    /// Query execution failed.
    #[error("execution error: {0}")]
    Execution(String),
}

/// A lightweight handle describing where a query lives in the compiled crate.
///
/// This is used internally by the driver so that `run` can lower and execute
/// the first query found in `main` without exposing HIR IDs in the public API.
#[derive(Debug, Clone, Copy)]
pub(crate) struct QueryLocation {
    pub(crate) body_id: BodyId,
    pub(crate) query_id: QueryId,
}
