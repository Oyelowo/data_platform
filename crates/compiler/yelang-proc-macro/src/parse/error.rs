//! Parse error type.

use std::fmt;

use crate::{Diagnostic, Span};

/// Error produced by a `Parse` implementation.
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub span: Span,
    pub message: String,
}

impl ParseError {
    /// Create a new parse error at `span` with the given message.
    pub fn new<M: Into<String>>(span: Span, message: M) -> Self {
        Self {
            span,
            message: message.into(),
        }
    }

    /// The span at which the error occurred.
    pub fn span(&self) -> Span {
        self.span
    }

    /// The error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Convert this error into a compiler diagnostic.
    pub fn to_diagnostic(&self) -> Diagnostic {
        Diagnostic::error(self.message.clone()).with_span(self.span)
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "parse error at {}: {}", self.span, self.message)
    }
}

impl std::error::Error for ParseError {}
