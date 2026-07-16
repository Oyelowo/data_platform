//! Low-level token cursor.

use std::marker::PhantomData;

use super::{Parse, ParseError};
use crate::{Span, TokenStream, TokenTree};

/// A cheaply cloneable cursor over a token stream.
///
/// `Cursor` owns a snapshot of the underlying `TokenStream` and an offset into
/// that snapshot. Forking is therefore a shallow clone of the vector plus an
/// integer copy, making speculative parsing inexpensive.
#[derive(Debug, Clone)]
pub struct Cursor<'a> {
    trees: Vec<TokenTree>,
    pos: usize,
    _marker: PhantomData<&'a TokenStream>,
}

impl<'a> Cursor<'a> {
    /// Create a cursor over `stream`.
    pub fn new(stream: &'a TokenStream) -> Self {
        Self {
            trees: stream.iter().collect(),
            pos: 0,
            _marker: PhantomData,
        }
    }

    /// Create a cursor from an already-collected vector of token trees.
    ///
    /// This is primarily used internally by `Parser` and `BufferedCursor`.
    pub(crate) fn from_trees(trees: Vec<TokenTree>) -> Self {
        Self {
            trees,
            pos: 0,
            _marker: PhantomData,
        }
    }

    /// The current token without consuming it.
    pub fn peek(&self) -> Option<&TokenTree> {
        self.trees.get(self.pos)
    }

    /// The `n`th token ahead of the current position, without consuming.
    pub fn peek_n(&self, n: usize) -> Option<&TokenTree> {
        self.trees.get(self.pos + n)
    }

    /// Consume and return the current token.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<TokenTree> {
        let result = self.trees.get(self.pos).cloned();
        if result.is_some() {
            self.pos += 1;
        }
        result
    }

    /// Advance the cursor by `n` tokens without returning them.
    pub fn advance(&mut self, n: usize) {
        self.pos = (self.pos + n).min(self.trees.len());
    }

    /// True if no tokens remain.
    pub fn is_empty(&self) -> bool {
        self.pos >= self.trees.len()
    }

    /// The number of tokens consumed so far.
    pub fn position(&self) -> usize {
        self.pos
    }

    /// The number of unconsumed tokens.
    pub fn remaining(&self) -> usize {
        self.trees.len().saturating_sub(self.pos)
    }

    /// The span of the current token, or `Span::call_site()` at EOF.
    pub fn span(&self) -> Span {
        self.peek()
            .map(|t| t.span())
            .unwrap_or_else(Span::call_site)
    }

    /// A cheap clone of this cursor for speculative parsing.
    pub fn fork(&self) -> Self {
        self.clone()
    }

    /// The stream of tokens that have not yet been consumed.
    pub fn remaining_stream(&self) -> TokenStream {
        self.trees[self.pos..].iter().cloned().collect()
    }

    /// Parse a value from the current cursor position.
    pub fn parse<T: Parse>(&mut self) -> Result<T, ParseError> {
        T::parse(self)
    }
}
