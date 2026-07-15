use std::fmt;

/// Error encountered during macro expansion.
#[derive(Debug, Clone, PartialEq)]
pub enum ExpandError {
    UnknownMacro {
        path: String,
        span: yelang_lexer::Span,
    },
    MalformedMacroArgs {
        reason: String,
        span: yelang_lexer::Span,
    },
    DecoratorError {
        reason: String,
        span: yelang_lexer::Span,
    },
    ExpansionLoop {
        path: String,
        span: yelang_lexer::Span,
    },
    MacroDefError {
        name: String,
        reason: String,
        span: yelang_lexer::Span,
    },
    MacroMatchError {
        name: String,
        reason: String,
        span: yelang_lexer::Span,
    },
    MacroTranscribeError {
        name: String,
        reason: String,
        span: yelang_lexer::Span,
    },
    AmbiguousMacro {
        name: String,
        span: yelang_lexer::Span,
    },
}

impl fmt::Display for ExpandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExpandError::UnknownMacro { path, .. } => write!(f, "unknown macro: {}", path),
            ExpandError::MalformedMacroArgs { reason, .. } => {
                write!(f, "malformed macro arguments: {}", reason)
            }
            ExpandError::DecoratorError { reason, .. } => write!(f, "decorator error: {}", reason),
            ExpandError::ExpansionLoop { path, .. } => {
                write!(f, "expansion loop detected: {}", path)
            }
            ExpandError::MacroDefError { name, reason, .. } => {
                write!(f, "error in macro definition `{}`: {}", name, reason)
            }
            ExpandError::MacroMatchError { name, reason, .. } => {
                write!(f, "macro `{}` could not match invocation: {}", name, reason)
            }
            ExpandError::MacroTranscribeError { name, reason, .. } => {
                write!(f, "macro `{}` transcription failed: {}", name, reason)
            }
            ExpandError::AmbiguousMacro { name, .. } => {
                write!(f, "macro `{}` invocation matches more than one rule", name)
            }
        }
    }
}

impl std::error::Error for ExpandError {}
