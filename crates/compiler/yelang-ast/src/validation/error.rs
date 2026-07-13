use yelang_lexer::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AstValidationError {
    pub message: String,
    pub span: Span,
}

impl AstValidationError {
    pub fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
        }
    }
}
