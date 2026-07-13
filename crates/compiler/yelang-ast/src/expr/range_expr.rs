/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 09/11/2025
 */

use crate::{T, expr::Expr};
use yelang_lexer::{ParseTokenStream, TokenResult, TokenStream};

#[derive(Debug, Clone, PartialEq)]
pub struct RangeExpr {
    pub start: Option<Box<Expr>>,
    pub op: RangeOp, // true for ..=, false for ..
    pub end: Option<Box<Expr>>,
}

impl RangeExpr {}

/// Range limits to distinguish `..` vs `..=`
#[derive(Debug, Clone, PartialEq)]
pub enum RangeOp {
    /// Closed range: `..=` (inclusive end)
    ///
    /// # Example
    /// ```
    /// 1..=10  // from 1 to 10
    /// ```
    Inclusive, // ..=
    /// Half-open range: `..` (exclusive end)
    ///
    /// # Example
    /// ```
    /// 1..10  // from 1 to 9
    /// ```
    Exclusive, // ..
}

impl RangeOp {
    /// Check if the range is inclusive
    pub fn is_inclusive(&self) -> bool {
        matches!(self, RangeOp::Inclusive)
    }

    /// Check if the range is exclusive
    pub fn is_exclusive(&self) -> bool {
        matches!(self, RangeOp::Exclusive)
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for RangeOp {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        use yelang_lexer::Either;

        let range_op = stream.parse::<Either<T![..=], T![..]>>()?;
        match range_op {
            Either::Left(_) => Ok(RangeOp::Inclusive),
            Either::Right(_) => Ok(RangeOp::Exclusive),
        }
    }
}
