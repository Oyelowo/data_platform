use yelang_macro_core::token_tree::{TokenStream, TokenTree};

/// A simple cursor over a `TokenStream` used by the matcher/parser and engine.
#[derive(Debug, Clone)]
pub struct TokenCursor {
    trees: Vec<TokenTree>,
    pos: usize,
}

impl TokenCursor {
    pub fn new(stream: TokenStream) -> Self {
        Self {
            trees: stream.into_iter().collect(),
            pos: 0,
        }
    }

    pub fn peek(&self) -> Option<&TokenTree> {
        self.trees.get(self.pos)
    }

    pub fn advance(&mut self) -> Option<TokenTree> {
        let tree = self.trees.get(self.pos).cloned()?;
        self.pos += 1;
        Some(tree)
    }

    pub fn is_eof(&self) -> bool {
        self.pos >= self.trees.len()
    }

    pub fn remaining(&self) -> &[TokenTree] {
        &self.trees[self.pos..]
    }
}

impl From<TokenStream> for TokenCursor {
    fn from(stream: TokenStream) -> Self {
        Self::new(stream)
    }
}
