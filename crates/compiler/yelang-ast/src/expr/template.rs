/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 07/12/2025
 */
use crate::{
    Expr, Symbol,
    expr::StringPart,
    tokens::{InterpolatedPart, TokenKind},
};
use yelang_lexer::{ParseTokenStream, TokenResult, TokenStream, consume_token};

#[derive(Debug, Clone, PartialEq)]
pub enum TemplatePart {
    /// Static string portion
    String(Symbol),
    /// Interpolated expression
    Expr(Box<Expr>),
}

/// Interpolated string expression parser
///
/// Parses interpolated strings like `"Hello ${name}"` into `Vec<StringPart>`
#[derive(Debug, Clone, PartialEq)]
pub struct InterpolatedStringExpr(pub Vec<StringPart>);

impl ParseTokenStream<crate::tokenizer::TokenKind> for InterpolatedStringExpr {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let interner = stream.interner().clone();
        let (parts, _) =
            consume_token!(stream, TokenKind::InterpolatedString { parts, kind } => (parts, kind));
        let mut string_parts = Vec::new();
        for part in parts {
            match part {
                InterpolatedPart::Literal(sym) => string_parts.push(StringPart::Literal(*sym)),
                InterpolatedPart::Expression(tokens) => {
                    // Create a temporary token stream for the expression
                    let mut expr_stream = TokenStream::<crate::tokenizer::TokenKind>::new_from_arc(
                        tokens.clone(),
                        interner.clone(),
                    );
                    let expr = expr_stream.parse::<Expr>()?;
                    string_parts.push(StringPart::Expr(Box::new(expr)));
                }
            }
        }
        Ok(InterpolatedStringExpr(string_parts))
    }
}
