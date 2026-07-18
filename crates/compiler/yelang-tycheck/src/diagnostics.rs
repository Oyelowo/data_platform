/*! User-facing diagnostics emitted by the type checker.
 *
 * Phase G introduces accumulating diagnostics. Each diagnostic carries a span
 * and a severity. In the future this can be extended with notes, labels, and
 * error codes.
 */

use yelang_infer::error::TypeError;
use yelang_lexer::Span;

/// Severity of a diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Note,
}

/// A single user-facing diagnostic.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub span: Span,
    pub message: String,
    pub severity: Severity,
}

impl Diagnostic {
    pub fn error(span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
            severity: Severity::Error,
        }
    }

    pub fn from_type_error(span: Span, err: &TypeError) -> Self {
        Self::error(span, err.to_string())
    }
}
