/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 11/13/2025
 */

use crate::expr::BlockExpr;
use crate::{T, TokenKind};
use yelang_lexer::{ParseTokenStream, Span, TokenResult, TokenStream};

/// Async expression
///
/// Represents async blocks and functions
///
/// # Example
/// ```
/// async { await fetch_data() }
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct AsyncExpr {
    // pub capture: CaptureBy,
    pub block: Box<BlockExpr>,
}

// /// Capture mode for async expressions
// #[derive(Debug, Clone, PartialEq)]
// pub enum CaptureBy {
//     /// Move capture: `async move { ... }`
//     Value,
//
//     /// Reference capture: `async { ... }`
//     Ref,
// }

impl ParseTokenStream<crate::tokenizer::TokenKind> for AsyncExpr {
    fn parse(stream: &mut TokenStream<TokenKind>) -> TokenResult<Self> {
        let async_ = stream.parse::<(T![async], BlockExpr)>()?;
        Ok(AsyncExpr {
            // capture: match async_.0 {
            //     T![move] => CaptureBy::Value,
            //     T![async] => CaptureBy::Ref,
            //     _ => unreachable!(),
            // },
            block: Box::new(async_.1),
        })
    }
}

impl AsyncExpr {}
