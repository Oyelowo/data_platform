/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 12/02/2025
 */
use crate::T;
use yelang_lexer::{ParseTokenStream, TokenResult, TokenStream};

use super::{Associativity, Expr, Precedence, PrecedenceExt};

#[derive(Debug, Clone, PartialEq)]
pub struct GroupedExpr {
    pub expr: Box<Expr>,
}

impl GroupedExpr {
    pub fn expr(&self) -> &Expr {
        &self.expr
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for GroupedExpr {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        // let (expr, span) = stream.parse_with_span::<(T!['('], Expr, T![')'])>()?;
        // let (expr, span) =
        //     stream.parse_with_span::<SurroundedBy<T!['('], Expr, T![')']>>()?;
        // Ok(GroupedExpr {
        //     expr: Box::new(expr.1),
        //     // expr: Box::new(expr.content_owned()),
        //     span,
        // })

        stream.parse::<T!['(']>()?;
        let expr = Expr::parse_pratt(stream, Precedence::None, super::Restrictions::NONE)?;
        stream.parse::<T![')']>()?;
        Ok(GroupedExpr {
            expr: Box::new(expr),
        })
    }
}

impl PrecedenceExt for GroupedExpr {
    fn precedence(&self) -> Precedence {
        Precedence::Primary
    }

    fn associativity(&self) -> Associativity {
        Associativity::Left
    }
}
