/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 21/02/2025
 */
use super::*;
use crate::Token;

pub trait Operator: Sized {
    fn precedence(&self) -> Precedence;
    fn parse(stream: &mut yelang_lexer::TokenStream<Token<'_>>) -> crate::lexer::TokenResult<Self>;
}

#[derive(Debug, Clone)]
pub enum InfixOperator {
    Comparison(ComparisonOp),
    Logical(LogicalOp),
    Arithmetic(ArithmeticOp),
}

impl Operator for InfixOperator<'_> {
    fn precedence(&self) -> Precedence {
        match self {
            Self::Logical(op) => match op {
                LogicalOp::Or => Precedence::LogicalOr,
                LogicalOp::And => Precedence::LogicalAnd,
                _ => Precedence::Lowest,
            },
            Self::Comparison(_) => Precedence::Comparison,
            Self::Arithmetic(op) => match op {
                ArithmeticOp::Plus | ArithmeticOp::Minus => Precedence::Term,
                ArithmeticOp::Multiply | ArithmeticOp::Divide => Precedence::Factor,
            },
        }
    }

    fn parse(stream: &mut yelang_lexer::TokenStream<Token<'_>>) -> crate::lexer::TokenResult<Self> {
        if let Ok(op) = stream.parse::<LogicalOp>() {
            return Ok(Self::Logical(op));
        }
        if let Ok(op) = stream.parse::<ComparisonOp>() {
            return Ok(Self::Comparison(op));
        }
        if let Ok(op) = stream.parse::<ArithmeticOp>() {
            return Ok(Self::Arithmetic(op));
        }
        Err(crate::lexer::TokenError::NoMatch)
    }
}
