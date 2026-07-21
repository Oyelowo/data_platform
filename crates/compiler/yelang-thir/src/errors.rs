//! THIR lowering errors.

use yelang_lexer::Span;

/// Errors produced while lowering HIR to THIR.
#[derive(Debug, Clone, thiserror::Error)]
pub enum LoweringError {
    #[error("method call has no resolved method")]
    UnresolvedMethodCall { span: Span },
    #[error("missing lang item: {0}")]
    MissingLangItem(String),
    #[error("unsupported HIR expression in THIR lowering")]
    Unsupported { message: String, span: Span },
}
