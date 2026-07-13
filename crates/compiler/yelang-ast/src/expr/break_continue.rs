/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2025 Oyelowo Oyedayo
 * Date 16/12/2025
 */

use crate::{Expr, Label, T};
use yelang_lexer::{ParseTokenStream, Span, TokenResult, TokenStream};

/// Break expression: `break`, `break value`, or `break 'label value`
///
/// Exits a loop, optionally with a label to target a specific loop,
/// and optionally with a value to return from the loop.
///
/// # Examples
/// ```
/// break;              // exit innermost loop
/// break 42;           // exit with value
/// break 'outer;       // exit labeled loop
/// break 'outer value; // exit labeled loop with value
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct BreakExpr {
    /// Optional label to target a specific loop
    pub label: Option<Label>,
    /// Optional value to return from the loop
    pub value: Option<Box<Expr>>,
    pub span: Span,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for BreakExpr {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let checkpoint = stream.checkpoint();
        let _break = stream.parse::<T![break]>()?;
        let label = stream.parse::<Option<Label>>().ok().flatten();
        let value = stream.parse::<Option<Expr>>().ok().flatten();
        let span = stream.span_since(checkpoint);

        Ok(BreakExpr {
            label,
            value: value.map(Box::new),
            span,
        })
    }
}

/// Continue expression: `continue` or `continue 'label`
///
/// Skips to the next iteration of a loop, optionally targeting
/// a specific loop by label.
///
/// # Examples
/// ```
/// continue;        // continue innermost loop
/// continue 'outer; // continue labeled loop
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ContinueExpr {
    /// Optional label to target a specific loop
    pub label: Option<Label>,
    pub span: Span,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for ContinueExpr {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let checkpoint = stream.checkpoint();
        let _continue = stream.parse::<T![continue]>()?;
        let label = stream.parse::<Option<Label>>().ok().flatten();
        let span = stream.span_since(checkpoint);

        Ok(ContinueExpr { label, span })
    }
}
