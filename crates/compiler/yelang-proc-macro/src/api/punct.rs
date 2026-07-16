//! Punctuation tokens.

use std::fmt;

use super::Span;

pub use yelang_macro_core::Spacing;

/// A single punctuation character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Punct {
    pub(crate) inner: yelang_macro_core::Punct,
}

impl Punct {
    /// Create a new punctuation token.
    pub fn new(ch: char, spacing: Spacing, span: Span) -> Self {
        Self {
            inner: yelang_macro_core::Punct::new(ch, spacing, span.into_inner()),
        }
    }

    /// The character value.
    pub fn as_char(&self) -> char {
        self.inner.ch
    }

    /// The spacing hint.
    pub fn spacing(&self) -> Spacing {
        self.inner.spacing
    }

    /// The span.
    pub fn span(&self) -> Span {
        Span::from_inner(self.inner.span)
    }

    /// Return a new punctuation token with the given span.
    pub fn with_span(self, span: Span) -> Self {
        Self::new(self.as_char(), self.spacing(), span)
    }
}

impl fmt::Display for Punct {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_char())
    }
}
