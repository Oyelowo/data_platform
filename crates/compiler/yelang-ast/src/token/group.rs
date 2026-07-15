use std::fmt;

use super::{Span, TokenId, TokenStream};

/// A delimited group of tokens.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Group {
    pub id: TokenId,
    pub delimiter: Delimiter,
    pub stream: TokenStream,
    pub span: Span,
}

impl Group {
    pub fn new(delimiter: Delimiter, stream: TokenStream, span: Span) -> Self {
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
