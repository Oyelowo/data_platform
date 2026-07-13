/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 31/12/2024
 */

use crate::{Attribute, Expr, Pattern, T, Type};
use yelang_lexer::{ParseTokenStream, Span, TokenResult, TokenStream};

/// Local variable binding: `let x = 42;`, `let mut y: i32 = 0;`, `let z;`, `let w: i32;`
///
/// Used in let statements within blocks and functions.
#[derive(Debug, Clone, PartialEq)]
pub struct LetStmt {
    pub pattern: Box<Pattern>,
    pub ty: Option<Box<Type>>,
    // TODO: Consider supporting LetElse and unit decl e.g let a;
    pub init: Option<Box<Expr>>,
    pub span: Span,
    pub attrs: Vec<Attribute>,
}

// e.g. let x = 10;
impl ParseTokenStream<crate::tokenizer::TokenKind> for LetStmt {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let checkpoint = stream.checkpoint();
        let attrs = stream
            .parse::<Option<crate::AttributesList>>()?
            .map(|a| a.0)
            .unwrap_or_default();
        stream.parse::<T![let]>()?;
        let pattern = stream.parse::<Pattern>()?;
        let ty = stream
            .parse::<Option<(T![:], Type)>>()?
            .map(|(_, ty)| Box::new(ty));
        let init = if stream.parse::<Option<T![=]>>()?.is_some() {
            Some(Box::new(stream.parse::<Expr>()?))
        } else {
            None
        };
        stream.parse::<T![;]>()?;
        let span = stream.span_since(checkpoint);
        Ok(LetStmt {
            pattern: Box::new(pattern),
            ty,
            init,
            span,
            attrs,
        })
    }
}
