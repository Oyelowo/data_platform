//! Parser trait and combinators.

use super::{Cursor, ParseError};
use crate::TokenStream;

/// Types that can be parsed from a token stream.
pub trait Parse: Sized {
    fn parse(input: &mut Cursor) -> Result<Self, ParseError>;
}

/// High-level parser entry point.
#[derive(Debug, Clone)]
pub struct Parser {
    stream: TokenStream,
}

impl Parser {
    pub fn new(stream: TokenStream) -> Self {
        Self { stream }
    }

    /// Parse a value from the stream.
    pub fn parse<T: Parse>(&mut self) -> Result<T, ParseError> {
        let mut cursor = Cursor::new(self.stream.clone());
        T::parse(&mut cursor)
    }
}

impl Parse for TokenStream {
    fn parse(input: &mut Cursor) -> Result<Self, ParseError> {
        let mut out = TokenStream::new();
        while let Some(tree) = input.next() {
            out.push(tree);
        }
        Ok(out)
    }
}
