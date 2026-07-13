/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 09/11/2025
 */

use crate::T;
use crate::{Pattern, expr::Expr};
use yelang_lexer::{ParseTokenStream, Span, TokenResult, TokenStream};

/// Match expression for pattern matching
///
/// # Example
/// ```
/// match value {
///     Some(x) if x > 0 => positive(x),
///     Some(x) => negative(x),
///     None => default(),
/// }
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct MatchExpr {
    /// The value being matched against
    pub scrutinee: Box<Expr>,
    /// Match arms with patterns and bodies
    pub arms: Vec<MatchArm>,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for MatchExpr {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        use crate::expr::ExprKind;
        use yelang_lexer::Verify;

        stream.parse::<T![match]>()?;
        let scrutinee = stream.parse::<Expr>()?;
        stream.parse::<T!['{']>()?;

        let mut arms = Vec::new();

        // Parse match arms with Rust's comma rules:
        // - Comma required after non-block expressions (except last arm)
        // - Comma optional after block expressions
        // - Trailing comma always optional
        while !stream.parse::<Verify<T!['}']>>().is_ok() {
            let arm = stream.parse::<MatchArm>()?;
            let is_block_like = arm.body.is_block_like();
            arms.push(arm);

            // Check if at end (for last arm - no comma needed)
            if stream.parse::<Verify<T!['}']>>().is_ok() {
                break;
            }

            // Try to parse comma
            let has_comma = stream.parse::<T![,]>().is_ok();

            // Rust's comma rules (matching rustc/rust-analyzer):
            // - After non-block-like expressions, comma is REQUIRED (except for last arm)
            // - After block-like expressions, comma is OPTIONAL
            if !is_block_like && !has_comma {
                // Non-block expression without comma - check if this is the last arm
                if !stream.parse::<Verify<T!['}']>>().is_ok() {
                    use yelang_lexer::TokenError;
                    return Err(TokenError::UnexpectedToken {
                        expected: "`,` after match arm (non-block expression)".to_string(),
                        found: stream.peek().map(|t| t.to_string()).unwrap_or_default(),
                        span: stream.current_span(),
                    });
                }
            }
        }

        stream.parse::<T!['}']>()?;

        Ok(MatchExpr {
            scrutinee: Box::new(scrutinee),
            arms,
        })
    }
}

/// A single arm in a match expression
///
/// # Example
/// ```
/// Some(x) if x > 0 => process(x)
/// None => default()
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    /// Pattern to match
    pub pattern: Pattern,
    /// Optional guard condition
    ///
    /// # Example
    /// ```
    /// Some(x) if x > 0 => ...
    /// ```
    pub guard: Option<Box<Expr>>,
    /// Expression to evaluate when pattern matches
    pub body: Box<Expr>,
    pub span: Span,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for MatchArm {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        type Arm = (Pattern, Option<(T![if], Expr)>, T!["=>"], Expr);

        let ((pattern, guard, _, body), span) = stream.parse_with_span::<Arm>()?;

        Ok(MatchArm {
            pattern,
            guard: guard.map(|(_if_, guard)| Box::new(guard)),
            body: Box::new(body),
            span,
        })
    }
}

impl MatchExpr {}

impl MatchArm {}

#[cfg(test)]
mod tests {
    use crate::Interner;

    use super::*;
    use crate::tokenizer::TokenKind;

    #[test]
    fn test_match_with_qualified_tuple_struct_pattern_and_nested_closure_body() {
        let input = r#"
            match opt {
                Option::Some(limit) => xs.probe_any(|y| xs.probe_any(|z| z > limit)),
                Option::None => false,
            }
        "#;

        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(input, &mut interner).unwrap();
        let match_expr = stream.parse::<MatchExpr>().unwrap();

        assert_eq!(match_expr.arms.len(), 2);
    }
}
