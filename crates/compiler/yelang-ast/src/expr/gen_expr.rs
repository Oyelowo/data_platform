/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 31/12/2024
 */
use crate::T;
use crate::expr::BlockExpr;
use yelang_lexer::{ParseTokenStream, TokenResult, TokenStream};

/// Generator expression: `gen { yield 1; }`
#[derive(Debug, Clone, PartialEq)]
pub struct GenExpr {
    pub block: Box<BlockExpr>,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for GenExpr {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let (_gen, block) = stream.parse::<(T![gen], BlockExpr)>()?;
        Ok(GenExpr {
            block: Box::new(block),
        })
    }
}
