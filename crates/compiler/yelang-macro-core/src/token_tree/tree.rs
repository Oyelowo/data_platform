use std::fmt;

use yelang_interner::Interner;

use super::{
    Span, TokenId,
    render::{render_group, render_ident, render_literal},
};

/// A single token or delimited group.
#[derive(Debug, Clone, PartialEq)]
pub enum TokenTree {
    Group(Group),
    Ident(super::Ident),
    Punct(super::Punct),
    Literal(super::Literal),
}

impl TokenTree {
    pub fn span(&self) -> Span {
        match self {
            TokenTree::Group(g) => g.span,
            TokenTree::Ident(i) => i.span,
            TokenTree::Punct(p) => p.span,
            TokenTree::Literal(l) => l.span,
        }
    }

    pub fn token_id(&self) -> TokenId {
        match self {
            TokenTree::Group(g) => g.id,
            TokenTree::Ident(i) => i.id,
            TokenTree::Punct(p) => p.id,
            TokenTree::Literal(l) => l.id,
        }
    }

    pub fn set_span(&mut self, span: Span) {
        match self {
            TokenTree::Group(g) => g.span = span,
            TokenTree::Ident(i) => i.span = span,
            TokenTree::Punct(p) => p.span = span,
            TokenTree::Literal(l) => l.span = span,
        }
    }
}

impl TokenTree {
    /// Render this token tree to a source string.
    pub fn render(&self, interner: &Interner) -> String {
        match self {
            TokenTree::Group(g) => render_group(g, interner),
            TokenTree::Ident(i) => render_ident(i, interner),
            TokenTree::Punct(p) => p.ch.to_string(),
            TokenTree::Literal(l) => render_literal(l, interner),
        }
    }
}

impl fmt::Display for TokenTree {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenTree::Group(g) => write!(f, "{}", g),
            TokenTree::Ident(i) => write!(f, "{}", i),
            TokenTree::Punct(p) => write!(f, "{}", p),
            TokenTree::Literal(l) => write!(f, "{}", l),
        }
    }
}

/// A delimited group of tokens.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Group {
    pub id: TokenId,
    pub delimiter: Delimiter,
    pub stream: super::TokenStream,
    pub span: Span,
}

impl Group {
    pub fn new(delimiter: Delimiter, stream: super::TokenStream, span: Span) -> Self {
        Self {
            id: TokenId::fresh(),
            delimiter,
            stream,
            span,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.stream.is_empty()
    }
}

impl fmt::Display for Group {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (open, close) = match self.delimiter {
            Delimiter::Parenthesis => ("(", ")"),
            Delimiter::Brace => ("{", "}"),
            Delimiter::Bracket => ("[", "]"),
            Delimiter::None => ("", ""),
        };
        write!(f, "{}{}{}", open, self.stream, close)
    }
}

/// The kind of delimiter around a `Group`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Delimiter {
    Parenthesis,
    Brace,
    Bracket,
    /// Invisible group used internally by macro expansion.
    #[default]
    None,
}
