/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 15/11/2025
 */

use crate::{BlockExpr, Expr, Label, T};
use yelang_lexer::{ParseTokenStream, TokenResult, TokenStream};

/// While loop expression: `while condition { body }` or `'label: while condition { body }`
///
/// A conditional loop that continues executing as long as the condition evaluates to true.
/// Can have an optional label for targeted break/continue statements.
///
/// # Example
/// ```
/// while x < 10 {
///     x += 1;
/// }
///
/// 'outer: while running {
///     // ...
/// }
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct WhileExpr {
    /// Optional label for break/continue targeting
    pub label: Option<Label>,
    /// The condition to evaluate before each iteration
    pub condition: Box<Expr>,
    /// The body of the loop containing statements and an optional final expression
    pub body: BlockExpr,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for WhileExpr {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let label = stream.parse::<Option<(Label, T![:])>>()?;
        stream.parse::<T![while]>()?;

        // Parse condition with struct literals forbidden to prevent ambiguity:
        // `while opt { ... }` should NOT parse `opt { ... }` as a struct literal
        let condition = Expr::parse_pratt(
            stream,
            super::Precedence::None,
            super::Restrictions::NO_STRUCT,
        )?;

        let body = stream.parse::<BlockExpr>()?;

        Ok(WhileExpr {
            label: label.map(|(label, _)| label),
            condition: Box::new(condition),
            body,
        })
    }
}
