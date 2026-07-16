//! Parser trait and combinators.

use super::{Cursor, ParseError};
use crate::{Delimiter, Group, Ident, Literal, Punct, TokenStream, TokenTree};

/// Types that can be parsed from a token stream.
pub trait Parse: Sized {
    /// Parse a value starting at the current cursor position.
    fn parse(cursor: &mut Cursor) -> Result<Self, ParseError>;
}

/// High-level parser handle.
///
/// `Parser` wraps a [`Cursor`] and provides ergonomic combinators for the
/// common case where a macro author wants to consume a stream piece by piece.
#[derive(Debug, Clone)]
pub struct Parser<'a> {
    cursor: Cursor<'a>,
}

impl<'a> Parser<'a> {
    /// Create a parser over `stream`.
    pub fn new(stream: &'a TokenStream) -> Self {
        Self {
            cursor: Cursor::new(stream),
        }
    }

    /// Create a parser from an existing cursor.
    pub fn from_cursor(cursor: Cursor<'a>) -> Self {
        Self { cursor }
    }

    /// Consume the parser and return the underlying cursor.
    pub fn into_cursor(self) -> Cursor<'a> {
        self.cursor
    }

    /// Borrow the underlying cursor.
    pub fn cursor(&self) -> &Cursor<'a> {
        &self.cursor
    }

    /// Borrow the underlying cursor mutably.
    pub fn cursor_mut(&mut self) -> &mut Cursor<'a> {
        &mut self.cursor
    }

    /// Parse a value from the stream.
    pub fn parse<T: Parse>(&mut self) -> Result<T, ParseError> {
        T::parse(&mut self.cursor)
    }

    /// The current token without consuming it.
    pub fn peek(&self) -> Option<&TokenTree> {
        self.cursor.peek()
    }

    /// The `n`th token ahead without consuming.
    pub fn peek_n(&self, n: usize) -> Option<&TokenTree> {
        self.cursor.peek_n(n)
    }

    /// True if no tokens remain.
    pub fn is_empty(&self) -> bool {
        self.cursor.is_empty()
    }

    /// Consume and return the current token.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<TokenTree> {
        self.cursor.next()
    }

    /// Consume the next token, returning an error if the stream is empty.
    pub fn expect(&mut self, expected: &str) -> Result<TokenTree, ParseError> {
        match self.cursor.next() {
            Some(tree) => Ok(tree),
            None => Err(ParseError::new(
                self.cursor.span(),
                format!("expected {}", expected),
            )),
        }
    }

    /// Consume the next token if it is an identifier.
    pub fn expect_ident(&mut self) -> Result<Ident, ParseError> {
        match self.cursor.next() {
            Some(TokenTree::Ident(i)) => Ok(i),
            Some(tree) => Err(ParseError::new(
                tree.span(),
                format!("expected identifier, found `{}`", tree),
            )),
            None => Err(ParseError::new(self.cursor.span(), "expected identifier")),
        }
    }

    /// Consume the next token if it is the identifier `name`.
    pub fn expect_keyword(&mut self, name: &str) -> Result<Ident, ParseError> {
        match self.cursor.next() {
            Some(TokenTree::Ident(i)) if i.value() == name => Ok(i),
            Some(tree) => Err(ParseError::new(
                tree.span(),
                format!("expected `{}`, found `{}`", name, tree),
            )),
            None => Err(ParseError::new(
                self.cursor.span(),
                format!("expected `{}`", name),
            )),
        }
    }

    /// True if the next token is the punctuation character `ch`.
    pub fn matches_punct(&self, ch: char) -> bool {
        matches!(self.cursor.peek(), Some(TokenTree::Punct(p)) if p.as_char() == ch)
    }

    /// True if the next token is an identifier with value `name`.
    pub fn matches_ident(&self, name: &str) -> bool {
        matches!(self.cursor.peek(), Some(TokenTree::Ident(i)) if i.value() == name)
    }

    /// Consume the next token if it is the punctuation character `ch`.
    ///
    /// Returns `true` if a token was consumed.
    pub fn consume_punct(&mut self, ch: char) -> bool {
        if self.matches_punct(ch) {
            self.cursor.next();
            true
        } else {
            false
        }
    }

    /// Consume the next token if it is an identifier with value `name`.
    ///
    /// Returns `true` if a token was consumed.
    pub fn consume_ident(&mut self, name: &str) -> bool {
        if self.matches_ident(name) {
            self.cursor.next();
            true
        } else {
            false
        }
    }

    /// Consume the next token if it is the punctuation character `ch`.
    pub fn expect_punct(&mut self, ch: char) -> Result<Punct, ParseError> {
        match self.cursor.next() {
            Some(TokenTree::Punct(p)) if p.as_char() == ch => Ok(p),
            Some(tree) => Err(ParseError::new(
                tree.span(),
                format!("expected `{}`, found `{}`", ch, tree),
            )),
            None => Err(ParseError::new(
                self.cursor.span(),
                format!("expected `{}`", ch),
            )),
        }
    }

    /// Consume the next token if it is a literal.
    pub fn expect_literal(&mut self) -> Result<Literal, ParseError> {
        match self.cursor.next() {
            Some(TokenTree::Literal(l)) => Ok(l),
            Some(tree) => Err(ParseError::new(
                tree.span(),
                format!("expected literal, found `{}`", tree),
            )),
            None => Err(ParseError::new(self.cursor.span(), "expected literal")),
        }
    }

    /// Consume the next token if it is a group with the given delimiter.
    pub fn expect_group(&mut self, delimiter: Delimiter) -> Result<Group, ParseError> {
        match self.cursor.next() {
            Some(TokenTree::Group(g)) if g.delimiter() == delimiter => Ok(g),
            Some(tree) => Err(ParseError::new(
                tree.span(),
                format!("expected {:?} group, found `{}`", delimiter, tree),
            )),
            None => Err(ParseError::new(
                self.cursor.span(),
                format!("expected {:?} group", delimiter),
            )),
        }
    }

    /// Try to parse `T`, restoring the cursor on failure.
    pub fn parse_optional<T: Parse>(&mut self) -> Option<T> {
        let snapshot = self.cursor.fork();
        match T::parse(&mut self.cursor) {
            Ok(value) => Some(value),
            Err(_) => {
                self.cursor = snapshot;
                None
            }
        }
    }

    /// Parse the next token tree as a group of any delimiter.
    pub fn parse_group(&mut self) -> Result<Group, ParseError> {
        Group::parse(&mut self.cursor)
    }

    /// Parse a sequence of `T` until `is_end` returns `true`.
    ///
    /// If `separator` is `Some(ch)`, a punctuation `ch` is expected between
    /// items. The terminator is *not* consumed.
    pub fn parse_terminated<T: Parse>(
        &mut self,
        mut is_end: impl FnMut(&TokenTree) -> bool,
        separator: Option<char>,
    ) -> Result<Vec<T>, ParseError> {
        let mut out = Vec::new();
        loop {
            if self.cursor.is_empty() {
                break;
            }
            if is_end(self.cursor.peek().unwrap()) {
                break;
            }
            out.push(T::parse(&mut self.cursor)?);
            if self.cursor.is_empty() {
                break;
            }
            if is_end(self.cursor.peek().unwrap()) {
                break;
            }
            if let Some(sep) = separator {
                self.expect_punct(sep)?;
            }
        }
        Ok(out)
    }

    /// Parse zero or more occurrences of `T`.
    pub fn parse_many0<T: Parse>(&mut self) -> Result<Vec<T>, ParseError> {
        let mut out = Vec::new();
        while !self.cursor.is_empty() {
            let snapshot = self.cursor.fork();
            match T::parse(&mut self.cursor) {
                Ok(value) => out.push(value),
                Err(_) => {
                    self.cursor = snapshot;
                    break;
                }
            }
        }
        Ok(out)
    }

    /// Parse one or more occurrences of `T`.
    pub fn parse_many1<T: Parse>(&mut self) -> Result<Vec<T>, ParseError> {
        let first = T::parse(&mut self.cursor)?;
        let mut out = vec![first];
        while !self.cursor.is_empty() {
            let snapshot = self.cursor.fork();
            match T::parse(&mut self.cursor) {
                Ok(value) => out.push(value),
                Err(_) => {
                    self.cursor = snapshot;
                    break;
                }
            }
        }
        Ok(out)
    }

    /// Parse zero or more `T` separated by `separator`.
    ///
    /// A trailing separator is allowed.
    pub fn parse_separated0<T: Parse>(&mut self, separator: char) -> Result<Vec<T>, ParseError> {
        let mut out = Vec::new();
        if self.cursor.is_empty() {
            return Ok(out);
        }
        let snapshot = self.cursor.fork();
        match T::parse(&mut self.cursor) {
            Ok(value) => out.push(value),
            Err(_) => {
                self.cursor = snapshot;
                return Ok(out);
            }
        }
        loop {
            if !self.consume_punct(separator) {
                break;
            }
            let snapshot = self.cursor.fork();
            match T::parse(&mut self.cursor) {
                Ok(value) => out.push(value),
                Err(_) => {
                    self.cursor = snapshot;
                    break;
                }
            }
        }
        Ok(out)
    }

    /// Parse one or more `T` separated by `separator`.
    ///
    /// A trailing separator is allowed.
    pub fn parse_separated1<T: Parse>(&mut self, separator: char) -> Result<Vec<T>, ParseError> {
        let mut out = Vec::new();
        out.push(T::parse(&mut self.cursor)?);
        loop {
            if !self.consume_punct(separator) {
                break;
            }
            let snapshot = self.cursor.fork();
            match T::parse(&mut self.cursor) {
                Ok(value) => out.push(value),
                Err(_) => {
                    self.cursor = snapshot;
                    break;
                }
            }
        }
        Ok(out)
    }

    /// Parse a group with the given delimiter and return a parser over its contents.
    pub fn parse_delimited(&mut self, delimiter: Delimiter) -> Result<Parser<'_>, ParseError> {
        let group = self.expect_group(delimiter)?;
        Ok(Parser::from_cursor(Cursor::from_trees(
            group.stream().iter().collect(),
        )))
    }
}

impl Parse for TokenStream {
    fn parse(cursor: &mut Cursor) -> Result<Self, ParseError> {
        Ok(cursor.remaining_stream())
    }
}

impl Parse for TokenTree {
    fn parse(cursor: &mut Cursor) -> Result<Self, ParseError> {
        match cursor.next() {
            Some(tree) => Ok(tree),
            None => Err(ParseError::new(cursor.span(), "expected token tree")),
        }
    }
}

impl Parse for Group {
    fn parse(cursor: &mut Cursor) -> Result<Self, ParseError> {
        match cursor.next() {
            Some(TokenTree::Group(g)) => Ok(g),
            Some(tree) => Err(ParseError::new(
                tree.span(),
                format!("expected group, found `{}`", tree),
            )),
            None => Err(ParseError::new(cursor.span(), "expected group")),
        }
    }
}

impl Parse for Ident {
    fn parse(cursor: &mut Cursor) -> Result<Self, ParseError> {
        match cursor.next() {
            Some(TokenTree::Ident(i)) => Ok(i),
            Some(tree) => Err(ParseError::new(
                tree.span(),
                format!("expected identifier, found `{}`", tree),
            )),
            None => Err(ParseError::new(cursor.span(), "expected identifier")),
        }
    }
}

impl Parse for Literal {
    fn parse(cursor: &mut Cursor) -> Result<Self, ParseError> {
        match cursor.next() {
            Some(TokenTree::Literal(l)) => Ok(l),
            Some(tree) => Err(ParseError::new(
                tree.span(),
                format!("expected literal, found `{}`", tree),
            )),
            None => Err(ParseError::new(cursor.span(), "expected literal")),
        }
    }
}

impl Parse for Punct {
    fn parse(cursor: &mut Cursor) -> Result<Self, ParseError> {
        match cursor.next() {
            Some(TokenTree::Punct(p)) => Ok(p),
            Some(tree) => Err(ParseError::new(
                tree.span(),
                format!("expected punctuation, found `{}`", tree),
            )),
            None => Err(ParseError::new(cursor.span(), "expected punctuation")),
        }
    }
}
