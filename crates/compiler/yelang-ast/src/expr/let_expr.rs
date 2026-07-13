/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 19/12/2024
 */

use crate::{Expr, Pattern, T};
use yelang_lexer::{ParseTokenStream, TokenResult, TokenStream};

/// Let expression for pattern matching: `let Some(x) = opt`
///
/// Used primarily in if-let and while-let conditions. When combined with `&&`,
/// creates let-chains. Let expressions have very low precedence - lower than
/// most binary operators including `&&`.
///
/// # Precedence
///
/// Let expressions cannot be nested arbitrarily. They have restricted placement:
/// - Can appear as the condition in `if` and `while`
/// - Can be combined with `&&` to form let-chains
/// - Cannot be used as operands to most other operators
///
/// # Examples
///
/// ```rust
/// // Simple let expression in if
/// if let Some(x) = opt {
///     println!("{}", x);
/// }
///
/// // Let-chains with && (parsed as binary expression)
/// if let Some(x) = opt && x > 5 {
///     println!("{}", x);
/// }
///
/// // Multiple lets in chain
/// if let Some(x) = opt && let Ok(y) = res {
///     println!("{}, {}", x, y);
/// }
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct LetExpr {
    /// The pattern to match against
    pub pattern: Pattern,
    /// The expression being matched
    pub expr: Box<Expr>,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for LetExpr {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let (_, pattern, _) = stream.parse::<(T![let], Pattern, T![=])>()?;

        // Parse RHS with struct literals forbidden to prevent ambiguity:
        // `let Some(x) = opt { x }` should NOT parse `opt { x }` as a struct literal
        //
        // Also use precedence level 5 (BitwiseOr = 4, LogicalAnd = 3):
        // This ensures `let x = a && b` stops at `&&` so the full expression becomes:
        // `(let x = a) && b` not `let x = (a && b)`
        //
        // Following rust-analyzer's precedence:
        // - LogicalAnd (&&) has precedence 4
        // - Let expression RHS uses precedence 5 (stops before &&)
        let expr = Expr::parse_pratt(
            stream,
            super::Precedence::BitwiseOr,
            super::Restrictions::NO_STRUCT,
        )?;

        Ok(LetExpr {
            pattern,
            expr: Box::new(expr),
        })
    }
}
