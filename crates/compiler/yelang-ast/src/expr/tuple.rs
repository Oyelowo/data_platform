/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 15/11/2025
 */
use crate::{Expr, T, TokenKind};
use yelang_lexer::{ParseTokenStream, SeparatedList, TokenResult, TokenStream};

#[derive(Debug, Clone, PartialEq)]
pub struct TupleExpr(pub Vec<Expr>);

impl ParseTokenStream<crate::tokenizer::TokenKind> for TupleExpr {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        stream.parse::<T!['(']>()?;

        // Handle the empty tuple `()` case
        if stream.parse::<Option<T![')']>>()?.is_some() {
            return Ok(TupleExpr(vec![]));
        }

        let mut exprs = stream
            .parse::<SeparatedList<Expr, T![,], true>>()?
            .value_owned();

        // Handle the single-element tuple `(expr,)` case
        if stream.parse::<Option<T![,]>>()?.is_some() {
            // A trailing comma was found.
        } else if exprs.len() == 1 {
            // This is a grouped expression, not a tuple. Fail the parse.
            return Err(yelang_lexer::TokenError::CustomError {
                msg: "Expected a tuple, found a grouped expression. Add a trailing comma for a single-element tuple.".into(),
                span: stream.span()
            });
        }

        stream.parse::<T![')']>()?;
        Ok(TupleExpr(exprs))
    }
}
