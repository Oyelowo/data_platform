/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 13/11/2025
 */

use crate::{BlockExpr, Label, T};
use yelang_lexer::{ParseTokenStream, TokenResult, TokenStream};

/// Loop expression: `loop { statements }`
///
/// An infinite loop that can return values via `break` expressions.
/// The loop runs indefinitely until a `break` is encountered.
///
/// # Example
/// ```
/// let result = loop {
///     let value = compute();
///     if value > 100 {
///         break value;
///     }
/// };
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct LoopExpr {
    /// The body of the loop containing statements and an optional final expression
    pub label: Option<Label>,
    pub body: Box<BlockExpr>,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for LoopExpr {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let (label, _, body) = stream.parse::<(Option<(Label, T![:])>, T![loop], BlockExpr)>()?;
        Ok(LoopExpr {
            label: label.map(|(label, _)| label),
            body: Box::new(body),
        })
    }
}

impl LoopExpr {}
