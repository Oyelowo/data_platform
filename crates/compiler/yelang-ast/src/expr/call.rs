/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 12/02/2025
 */
use crate::{Expr, Ident, T};
use yelang_lexer::{ArrayCreator, ParseTokenStream, TokenResult, TokenStream, Verify};

// Calls
/// Function call: `foo(1, 2)`
///
/// # Example
/// ```
/// add(1, 2)
/// println("hello")
/// ```
/// Enum variant construction
///
/// # Example
/// ```
/// Option::Some(42)
/// Result::Ok("success")
/// Status::Active
/// ```
// e.g math::sum(1, 2) or map(math::sum)
// math::sum<T>(1, 2) or map(math::sum<T>)
// math::sum<path::Wrap<path::Flow>>(1, 2) or map(math::sum::<path::Wrap<path::Flow>>)
// math::sum::<path::Wrap<path::Flow>>(1, 2) or map(math::sum::<path::Wrap<path::Flow>>)
#[derive(Debug, Clone, PartialEq)]
pub struct CallExpr {
    pub callee: Box<Expr>,
    pub args: Vec<CallArgument>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CallArgument {
    Positional(Expr),
    Named(Ident, Expr),
}

impl CallArgument {
    pub fn is_positional(&self) -> bool {
        matches!(self, CallArgument::Positional(_))
    }

    pub fn is_named(&self) -> bool {
        matches!(self, CallArgument::Named(_, _))
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for CallArgument {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        // Deterministic parse with lookahead:
        // - If we see `ident = ...`, parse as named argument.
        // - Otherwise parse as positional expression.
        //
        // This avoids relying on generic `Either` backtracking, which can be
        // sensitive to partial consumption in complex call sites.
        let checkpoint = stream.checkpoint();

        if let Ok(ident) = stream.parse::<Ident>() {
            let after_ident = stream.checkpoint();
            if stream.parse::<Verify<T![=]>>().is_ok() {
                stream.restore(after_ident);
                let _eq = stream.parse::<T![=]>()?;
                let expr = stream.parse::<Expr>()?;
                return Ok(CallArgument::Named(ident, expr));
            }

            // Not a named arg; restore and fall through to positional parsing.
            stream.restore(checkpoint);
        } else {
            stream.restore(checkpoint);
        }

        let expr = stream.parse::<Expr>()?;
        Ok(CallArgument::Positional(expr))
    }
}

pub type CallArgs = ArrayCreator<T!['('], CallArgument, T![,], T![')']>;

impl ParseTokenStream<crate::tokenizer::TokenKind> for CallExpr {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let checkpoint = stream.checkpoint();

        let callee = stream.parse::<Expr>()?;
        let (args, span) = stream.parse_with_span::<CallArgs>()?;

        Ok(CallExpr {
            callee: Box::new(callee),
            args: args.items_owned(),
        })
    }
}

impl CallExpr {}
