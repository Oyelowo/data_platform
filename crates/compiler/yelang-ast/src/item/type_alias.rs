/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 11/12/2025
 */

use super::generics::GenericParamsParser;
use super::{Generics, WhereClause};
use crate::{Ident, T, Type};
use yelang_lexer::{ParseTokenStream, Span, TokenResult, TokenStream};

/// Type alias definition
///
/// # Example
/// ```
/// type Result<T> = std::result::Result<T, Error>;
/// type Point = (i32, i32);
/// type UserMap = HashMap<string, User>;
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct TypeAlias {
    /// Alias name
    pub name: Ident,
    /// Generic parameters
    pub generics: Generics,
    /// The type being aliased
    pub target: Type,
    pub span: Span,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for TypeAlias {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let checkpoint = stream.checkpoint();

        // type Name <T> = Target where ... ;
        type AliasTuple = (
            T![type],
            Ident,
            Option<GenericParamsParser>, // <T>
            T![=],
            Type,
            Option<WhereClause>, // where ...
            T![;],
        );

        let (_type, name, gen_params, _eq, target, where_clause, _semi) =
            stream.parse::<AliasTuple>()?;

        let generics = Generics {
            params: gen_params.map(|g| g.items_owned()).unwrap_or_default(),
            where_clause,
            span: stream.span_since(checkpoint),
        };

        Ok(TypeAlias {
            name,
            generics,
            target,
            span: stream.span_since(checkpoint),
        })
    }
}
