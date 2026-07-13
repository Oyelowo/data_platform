/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 20/11/2025
 */

use crate::{
    AssignOpKind, BinaryOp, RangeOp, T,
    expr::{Associativity, Precedence, PrecedenceExt},
};
use yelang_lexer::{ParseTokenStream, TokenResult, TokenStream, match_map};

#[derive(Debug, Clone, PartialEq)]
pub enum InfixOp {
    Binary(BinaryOp),
    AssignEq,               // Simple assignment `=`
    AssignOp(AssignOpKind), // Compound assignment `+=`, `-=`, etc.
    Range(RangeOp),         // `..` or `..=`
}

impl PrecedenceExt for InfixOp {
    fn precedence(&self) -> Precedence {
        match self {
            InfixOp::Binary(op) => op.precedence(),
            InfixOp::AssignEq | InfixOp::AssignOp(_) => Precedence::Assignment,
            InfixOp::Range { .. } => Precedence::Range,
        }
    }

    fn associativity(&self) -> Associativity {
        match self {
            InfixOp::Binary(op) => op.associativity(),
            InfixOp::AssignEq | InfixOp::AssignOp(_) => Associativity::Right,
            InfixOp::Range { .. } => Associativity::Left, // Ranges are typically left-associative
        }
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for InfixOp {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let res = match_map!(
            stream,
            // Try assignment operators first (highest precedence check)
            AssignOpKind => InfixOp::AssignOp,
            BinaryOp => InfixOp::Binary,
            RangeOp => InfixOp::Range,
            T![=] => |_| InfixOp::AssignEq

        )?;
        Ok(res)
    }
}
