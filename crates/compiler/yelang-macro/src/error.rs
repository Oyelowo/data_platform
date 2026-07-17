use std::fmt;

/// Severity level of a diagnostic emitted by a procedural macro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticLevel {
    Error,
    Warning,
    Note,
    Help,
}

/// A single frame in a macro expansion backtrace.
#[derive(Debug, Clone, PartialEq)]
pub struct BacktraceFrame {
    pub name: String,
    pub span: yelang_lexer::Span,
}

/// Error encountered during macro expansion.
#[derive(Debug, Clone, PartialEq)]
pub enum ExpandError {
    UnknownMacro {
        path: String,
        span: yelang_lexer::Span,
        backtrace: Vec<BacktraceFrame>,
    },
    MalformedMacroArgs {
        reason: String,
        span: yelang_lexer::Span,
        backtrace: Vec<BacktraceFrame>,
    },
    DecoratorError {
        reason: String,
        span: yelang_lexer::Span,
        backtrace: Vec<BacktraceFrame>,
    },
    ExpansionLoop {
        path: String,
        span: yelang_lexer::Span,
        backtrace: Vec<BacktraceFrame>,
    },
    RecursionLimit {
        name: String,
        span: yelang_lexer::Span,
        backtrace: Vec<BacktraceFrame>,
    },
    MacroDefError {
        name: String,
        reason: String,
        span: yelang_lexer::Span,
        backtrace: Vec<BacktraceFrame>,
    },
    MacroMatchError {
        name: String,
        reason: String,
        span: yelang_lexer::Span,
        backtrace: Vec<BacktraceFrame>,
    },
    MacroTranscribeError {
        name: String,
        reason: String,
        span: yelang_lexer::Span,
        backtrace: Vec<BacktraceFrame>,
    },
    AmbiguousMacro {
        name: String,
        span: yelang_lexer::Span,
        backtrace: Vec<BacktraceFrame>,
    },
    ProcMacroDiagnostic {
        level: DiagnosticLevel,
        message: String,
        span: yelang_lexer::Span,
        backtrace: Vec<BacktraceFrame>,
    },
}

impl ExpandError {
    /// Replace the backtrace on this error.
    pub fn with_backtrace(mut self, backtrace: Vec<BacktraceFrame>) -> Self {
        match &mut self {
            ExpandError::UnknownMacro { backtrace: bt, .. }
            | ExpandError::MalformedMacroArgs { backtrace: bt, .. }
            | ExpandError::DecoratorError { backtrace: bt, .. }
            | ExpandError::ExpansionLoop { backtrace: bt, .. }
            | ExpandError::RecursionLimit { backtrace: bt, .. }
            | ExpandError::MacroDefError { backtrace: bt, .. }
            | ExpandError::MacroMatchError { backtrace: bt, .. }
            | ExpandError::MacroTranscribeError { backtrace: bt, .. }
            | ExpandError::AmbiguousMacro { backtrace: bt, .. }
            | ExpandError::ProcMacroDiagnostic { backtrace: bt, .. } => {
                *bt = backtrace;
            }
        }
        self
    }

    pub fn span(&self) -> yelang_lexer::Span {
        match self {
            ExpandError::UnknownMacro { span, .. }
            | ExpandError::MalformedMacroArgs { span, .. }
            | ExpandError::DecoratorError { span, .. }
            | ExpandError::ExpansionLoop { span, .. }
            | ExpandError::RecursionLimit { span, .. }
            | ExpandError::MacroDefError { span, .. }
            | ExpandError::MacroMatchError { span, .. }
            | ExpandError::MacroTranscribeError { span, .. }
            | ExpandError::AmbiguousMacro { span, .. }
            | ExpandError::ProcMacroDiagnostic { span, .. } => *span,
        }
    }
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
            ExpandError::RecursionLimit { name, .. } => {
                write!(f, "recursion limit exceeded while expanding `{}`", name)
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
            ExpandError::ProcMacroDiagnostic { level, message, .. } => {
                let level_str = match level {
                    DiagnosticLevel::Error => "error",
                    DiagnosticLevel::Warning => "warning",
                    DiagnosticLevel::Note => "note",
                    DiagnosticLevel::Help => "help",
                };
                write!(f, "proc macro {}: {}", level_str, message)
            }
        }
    }
}

impl ExpandError {
    pub fn unknown_macro(path: impl Into<String>, span: yelang_lexer::Span) -> Self {
        ExpandError::UnknownMacro {
            path: path.into(),
            span,
            backtrace: vec![],
        }
    }

    pub fn malformed_macro_args(reason: impl Into<String>, span: yelang_lexer::Span) -> Self {
        ExpandError::MalformedMacroArgs {
            reason: reason.into(),
            span,
            backtrace: vec![],
        }
    }

    pub fn decorator_error(reason: impl Into<String>, span: yelang_lexer::Span) -> Self {
        ExpandError::DecoratorError {
            reason: reason.into(),
            span,
            backtrace: vec![],
        }
    }

    pub fn expansion_loop(path: impl Into<String>, span: yelang_lexer::Span) -> Self {
        ExpandError::ExpansionLoop {
            path: path.into(),
            span,
            backtrace: vec![],
        }
    }

    pub fn recursion_limit(name: impl Into<String>, span: yelang_lexer::Span) -> Self {
        ExpandError::RecursionLimit {
            name: name.into(),
            span,
            backtrace: vec![],
        }
    }

    pub fn macro_def_error(
        name: impl Into<String>,
        reason: impl Into<String>,
        span: yelang_lexer::Span,
    ) -> Self {
        ExpandError::MacroDefError {
            name: name.into(),
            reason: reason.into(),
            span,
            backtrace: vec![],
        }
    }

    pub fn macro_match_error(
        name: impl Into<String>,
        reason: impl Into<String>,
        span: yelang_lexer::Span,
    ) -> Self {
        ExpandError::MacroMatchError {
            name: name.into(),
            reason: reason.into(),
            span,
            backtrace: vec![],
        }
    }

    pub fn macro_transcribe_error(
        name: impl Into<String>,
        reason: impl Into<String>,
        span: yelang_lexer::Span,
    ) -> Self {
        ExpandError::MacroTranscribeError {
            name: name.into(),
            reason: reason.into(),
            span,
            backtrace: vec![],
        }
    }

    pub fn ambiguous_macro(name: impl Into<String>, span: yelang_lexer::Span) -> Self {
        ExpandError::AmbiguousMacro {
            name: name.into(),
            span,
            backtrace: vec![],
        }
    }

    pub fn proc_macro_diagnostic(
        level: DiagnosticLevel,
        message: impl Into<String>,
        span: yelang_lexer::Span,
    ) -> Self {
        ExpandError::ProcMacroDiagnostic {
            level,
            message: message.into(),
            span,
            backtrace: vec![],
        }
    }
}

impl std::error::Error for ExpandError {}
