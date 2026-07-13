/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 12/02/2025
 */
use crate::{Expr, Ident, T};
use yelang_lexer::{ParseTokenStream, Span, TokenResult, TokenStream};

#[derive(Debug, Clone, PartialEq)]
pub struct IdExpr {
    pub table: Ident,
    pub value: Expr,
    pub span: Span,
}

impl IdExpr {
    pub fn table(&self) -> &Ident {
        &self.table
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for IdExpr {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let ((ident, _, val), span) = stream.parse_with_span::<(Ident, T![:], Expr)>()?;

        Ok(Self {
            table: ident,
            value: val,
            span,
        })
    }
}
