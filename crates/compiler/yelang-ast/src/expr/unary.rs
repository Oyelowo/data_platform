/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 12/02/2025
 */
use crate::T;
use yelang_lexer::{Either, ParseTokenStream, match_map};

use super::{Associativity, Expr, Precedence, PrecedenceExt};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum UnaryOp {
    /// !expr
    Not,
    /// -expr
    Neg,
    /// *expr
    Deref,
    /// &expr
    Ref,
    /// &mut expr
    RefMut,
}

impl PrecedenceExt for UnaryOp {
    fn precedence(&self) -> Precedence {
        Precedence::Unary
    }

    fn associativity(&self) -> Associativity {
        Associativity::Right
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for UnaryOp {
    fn parse(
        stream: &mut yelang_lexer::TokenStream<crate::tokenizer::TokenKind>,
    ) -> yelang_lexer::TokenResult<Self> {
        match_map!(
            stream,
            T![!] => |_| Self::Not,
            T![-] => |_| Self::Neg,
            T![*] => |_| Self::Deref,
            T![&] => |_| Self::Ref,
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct UnaryExpr {
    pub op: UnaryOp,
    pub expr: Box<Expr>,
}

impl UnaryExpr {
    pub fn op(&self) -> UnaryOp {
        self.op
    }

    pub fn expr(&self) -> &Expr {
        &self.expr
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for UnaryExpr {
    fn parse(
        stream: &mut yelang_lexer::TokenStream<crate::tokenizer::TokenKind>,
    ) -> yelang_lexer::TokenResult<Self> {
        let checkpoint = stream.checkpoint();

        let op = stream.parse::<UnaryOp>()?;
        let expr = Expr::parse_pratt(stream, Precedence::Unary, super::Restrictions::NONE)?;

        let _span = stream.span_since(checkpoint);
        Ok(Self {
            op,
            expr: Box::new(expr),
        })
    }
}
