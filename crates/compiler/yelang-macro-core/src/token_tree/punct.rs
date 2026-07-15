use std::fmt;

use super::{Span, TokenId};

/// A punctuation token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Punct {
    pub id: TokenId,
    pub ch: char,
    pub spacing: Spacing,
    pub span: Span,
}

impl Punct {
    pub fn new(ch: char, spacing: Spacing, span: Span) -> Self {
        Self {
            id: TokenId::fresh(),
            ch,
            spacing,
            span,
        }
    }

    pub fn alone(ch: char, span: Span) -> Self {
        Self::new(ch, Spacing::Alone, span)
    }

    pub fn joint(ch: char, span: Span) -> Self {
        Self::new(ch, Spacing::Joint, span)
    }
}

impl fmt::Display for Punct {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.ch)
    }
}

/// Whether this punctuation character can potentially be combined with the
/// following punctuation character to form a multi-character operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spacing {
    /// This punctuation character may be combined with the next one.
    Joint,
    /// This punctuation character cannot be combined with the next one.
    Alone,
}
