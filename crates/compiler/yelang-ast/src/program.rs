use yelang_lexer::{ParseTokenStream, RepeatMin, Span, TokenError, TokenResult, TokenStream};

use super::item::Item;

/// Complete program
///
/// # Example
/// ```
/// use std::io;
///
/// struct Point { x: i32, y: i32 }
///
/// fn main() {
///     let p = Point { x: 0, y: 0 };
/// }
/// ```
#[derive(Debug, Clone)]
pub struct Program {
    /// Top-level items
    pub items: Vec<Item>,
    pub span: Span,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for Program {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let (items, span) = stream.parse_with_span::<RepeatMin<0, Item>>()?;

        // `RepeatMin<0, Item>` can succeed even if it stops early.
        // Make `Program` parsing strict so syntax errors aren't silently dropped.
        if !stream.is_eof() {
            let checkpoint = stream.checkpoint();
            let next_item_err = stream.parse::<Item>().err();
            stream.restore(checkpoint);

            if let Some(e) = next_item_err {
                return Err(e);
            }

            let token = stream
                .peek()
                .map(|t| format!("{}", t.kind()))
                .unwrap_or_else(|| "<eof>".to_string());
            return Err(TokenError::SyntaxError {
                message: format!("unexpected trailing tokens starting at {token}"),
                span: stream.current_span(),
                source: None,
            });
        }

        Ok(Program {
            items: items.value_owned(),
            span,
        })
    }
}
