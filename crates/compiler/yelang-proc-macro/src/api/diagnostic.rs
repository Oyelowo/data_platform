//! Diagnostics emitted by procedural macros.

use super::Span;

/// Severity level of a diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Error,
    Warning,
    Note,
    Help,
}

/// A compiler diagnostic produced by a procedural macro.
#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub level: Level,
    pub message: String,
    pub spans: Vec<DiagnosticSpan>,
}

/// A labeled span inside a diagnostic.
#[derive(Debug, Clone, PartialEq)]
pub struct DiagnosticSpan {
    pub span: Span,
    pub label: Option<String>,
}

impl Diagnostic {
    /// Create a new error diagnostic.
    pub fn error<M: Into<String>>(message: M) -> Self {
        Self {
            level: Level::Error,
            message: message.into(),
            spans: Vec::new(),
        }
    }

    /// Add a primary span.
    pub fn with_span(mut self, span: Span) -> Self {
        self.spans.push(DiagnosticSpan { span, label: None });
        self
    }

    /// Add a labeled span.
    pub fn with_labeled_span<M: Into<String>>(mut self, span: Span, label: M) -> Self {
        self.spans.push(DiagnosticSpan {
            span,
            label: Some(label.into()),
        });
        self
    }

    /// Emit the diagnostic through the current macro context.
    pub fn emit(self) {
        thread_local! {
            static DIAGNOSTICS: std::cell::RefCell<Vec<Diagnostic>> =
                std::cell::RefCell::new(Vec::new());
        }
        DIAGNOSTICS.with(|d| d.borrow_mut().push(self));
    }
}

/// Drain diagnostics emitted in the current thread.
pub fn drain_diagnostics() -> Vec<Diagnostic> {
    thread_local! {
        static DIAGNOSTICS: std::cell::RefCell<Vec<Diagnostic>> =
            std::cell::RefCell::new(Vec::new());
    }
    DIAGNOSTICS.with(|d| std::mem::take(&mut *d.borrow_mut()))
}
