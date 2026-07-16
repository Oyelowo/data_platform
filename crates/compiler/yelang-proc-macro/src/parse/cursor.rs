//! Low-level token cursor.

use crate::{Span, TokenStream, TokenTree};

/// A cursor over a token stream that allows peeking.
#[derive(Debug, Clone)]
pub struct Cursor {
    trees: Vec<TokenTree>,
    pos: usize,
}

impl Cursor {
    /// Create a cursor over a stream.
    pub fn new(stream: TokenStream) -> Self {
        Self {
            trees: stream.iter().collect(),
            pos: 0,
        }
    }

    /// The current token without consuming it.
    pub fn peek(&self) -> Option<&TokenTree> {
        self.trees.get(self.pos)
    }

    /// Consume and return the current token.
    pub fn next(&mut self) -> Option<TokenTree> {
        let result = self.trees.get(self.pos).cloned();
        if result.is_some() {
            self.pos += 1;
        }
        result
    }

    /// True if no tokens remain.
    pub fn is_empty(&self) -> bool {
        self.peek().is_none()
    }

    /// The span of the current token, or `Span::call_site()` if empty.
    pub fn span(&self) -> Span {
        self.peek()
            .map(|t| t.span())
            .unwrap_or_else(Span::call_site)
    }
}
