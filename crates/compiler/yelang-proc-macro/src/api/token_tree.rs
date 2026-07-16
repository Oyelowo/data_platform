//! Token tree and group types.

use std::fmt;

use super::{Delimiter, Ident, Literal, Punct, Span, TokenStream};

/// A single token or a delimited group of tokens.
#[derive(Debug, Clone, PartialEq)]
pub enum TokenTree {
    Group(Group),
    Ident(Ident),
    Punct(Punct),
    Literal(Literal),
}

impl TokenTree {
    /// The span of this token tree.
    pub fn span(&self) -> Span {
        match self {
            TokenTree::Group(g) => g.span(),
            TokenTree::Ident(i) => i.span(),
            TokenTree::Punct(p) => p.span(),
            TokenTree::Literal(l) => l.span(),
        }
    }

    /// Return a new token tree with the given span.
    pub fn with_span(self, span: Span) -> Self {
        match self {
            TokenTree::Group(g) => TokenTree::Group(g.with_span(span)),
            TokenTree::Ident(i) => TokenTree::Ident(i.with_span(span)),
            TokenTree::Punct(p) => TokenTree::Punct(p.with_span(span)),
            TokenTree::Literal(l) => TokenTree::Literal(l.with_span(span)),
        }
    }

    pub(crate) fn into_inner(self) -> yelang_macro_core::TokenTree {
        match self {
            TokenTree::Group(g) => yelang_macro_core::TokenTree::Group(g.into_inner()),
            TokenTree::Ident(i) => yelang_macro_core::TokenTree::Ident(i.inner),
            TokenTree::Punct(p) => yelang_macro_core::TokenTree::Punct(p.inner),
            TokenTree::Literal(l) => yelang_macro_core::TokenTree::Literal(l.inner),
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

/// A delimited group of token trees.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub(crate) id: yelang_macro_core::TokenId,
    pub(crate) delimiter: Delimiter,
    pub(crate) stream: TokenStream,
    pub(crate) span: Span,
}

impl Group {
    /// Create a new group.
    pub fn new(delimiter: Delimiter, stream: TokenStream, span: Span) -> Self {
        Self {
            id: yelang_macro_core::TokenId::fresh(),
            delimiter,
            stream,
            span,
        }
    }

    /// The delimiter.
    pub fn delimiter(&self) -> Delimiter {
        self.delimiter
    }

    /// The inner stream.
    pub fn stream(&self) -> TokenStream {
        self.stream.clone()
    }

    /// The span.
    pub fn span(&self) -> Span {
        self.span
    }

    /// Return a new group with the given span.
    pub fn with_span(self, span: Span) -> Self {
        Self { span, ..self }
    }

    pub(crate) fn into_inner(self) -> yelang_macro_core::Group {
        yelang_macro_core::Group {
            id: self.id,
            delimiter: self.delimiter,
            stream: self.stream.into_core_stream(),
            span: self.span.into_inner(),
        }
    }
}

impl fmt::Display for Group {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (open, close) = match self.delimiter() {
            Delimiter::Parenthesis => ("(", ")"),
            Delimiter::Brace => ("{", "}"),
            Delimiter::Bracket => ("[", "]"),
            Delimiter::None => ("", ""),
        };
        write!(f, "{}{}{}", open, self.stream(), close)
    }
}
