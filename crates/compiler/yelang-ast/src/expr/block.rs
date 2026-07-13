/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 31/12/2024
 */

use crate::{Ident, Stmt, T};
use yelang_interner::Symbol;
use yelang_lexer::{ParseTokenStream, RepeatMin, Span, TokenResult, TokenStream};

#[derive(Debug, Clone, PartialEq)]
pub struct BlockExpr {
    pub label: Option<Label>,
    pub statements: Vec<Stmt>,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for BlockExpr {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        use crate::TokenKind;

        let label = stream.parse::<Option<(Label, T![:])>>()?.map(|(l, _)| l);
        stream.parse::<T!['{']>()?;

        let mut statements = Vec::new();
        loop {
            let is_close = matches!(stream.peek().map(|t| t.kind()), Some(TokenKind::CloseBrace));
            if is_close {
                break;
            }
            statements.push(stream.parse::<Stmt>()?);
        }

        stream.parse::<T!['}']>()?;

        Ok(BlockExpr { label, statements })
    }
}
/// Label for labeled blocks and loops
///
/// # Example
/// ```
/// 'outer: loop { ... }
/// 'my_block: { ... }
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    pub symbol: Symbol,
    pub span: Span,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for Label {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        // Parse lifetime token (e.g., 'outer) followed by colon
        use crate::tokenizer::TokenKind;
        use yelang_lexer::consume_token;

        let checkpoint = stream.checkpoint();

        let symbol = *consume_token!(stream, TokenKind::Lifetime(s) => s);
        let span = stream.span_since(checkpoint);

        Ok(Label { symbol, span })
    }
}
