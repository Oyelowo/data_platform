//! Parse error type.

use crate::Span;

/// Error produced by a `Parse` implementation.
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub span: Span,
    pub message: String,
}

impl ParseError {
    pub fn new<M: Into<String>>(span: Span, message: M) -> Self {
        Self {
            span,
            message: message.into(),
        }
    }
}
