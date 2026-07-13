/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 09/02/2025
 */
use crate::{Expr, T};
use yelang_lexer::{ArrayCreator, ParseTokenStream, TokenResult, TokenStream, match_map};

#[derive(Debug, Clone, PartialEq)]
pub enum ArrayKind {
    /// List of elements: [1, 2, 3]
    List(Vec<Expr>),
    /// Repeat syntax: [value; count]
    Repeat { value: Box<Expr>, count: Box<Expr> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Array {
    pub kind: ArrayKind,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for Array {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        // Try parsing repeat syntax first: [value; count]
        // Then fall back to list syntax: [elem1, elem2, ...]

        let kind = match_map!(
            stream,
            (T!['['], Expr, T![;], Expr, T![']']) => |(_, value, _, count, _)| {
                ArrayKind::Repeat {
                    value: Box::new(value),
                    count: Box::new(count),
                }
            },
            ArrayCreator<T!['['], Expr, T![,], T![']']> => |array| {
                ArrayKind::List(array.items_owned())
            },
        )?;

        Ok(Array { kind })
    }
}

impl Array {
    pub fn kind(&self) -> &ArrayKind {
        &self.kind
    }

    /// Get elements if this is a list array, None if repeat array
    pub fn elements(&self) -> Option<&[Expr]> {
        match &self.kind {
            ArrayKind::List(elems) => Some(elems),
            ArrayKind::Repeat { .. } => None,
        }
    }
}
