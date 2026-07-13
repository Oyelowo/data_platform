/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 11/12/2025
 */

use super::generics::GenericParamsParser;
use super::*;
use crate::{Ident, T};
use yelang_lexer::{
    ParseTokenStream, RepeatMin, SeparatedList, Span, TokenResult, TokenStream, match_map,
};

/// Trait definition
///
/// # Example
/// ```
/// trait Display {
///     fn fmt(&self) -> string;
/// }
///
/// trait Iterator<T> {
///     type Item;
///     fn next(&mut self) -> Option<Self::Item>;
/// }
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Trait {
    /// Trait name
    pub name: Ident,
    /// Generic parameters
    pub generics: Generics,
    /// Super traits (trait bounds)
    pub super_traits: Vec<TraitBound>,
    /// Trait items (methods, associated types, constants)
    pub items: Vec<TraitItem>,
    pub span: Span,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for Trait {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let checkpoint = stream.checkpoint();

        let ((_, name, gen_params, where_clause, super_traits, _, items, _), span) = stream
            .parse_with_span::<(
                T![trait],
                Ident,
                Option<GenericParamsParser>,
                Option<WhereClause>,
                Option<(T![:], SeparatedList<TraitBound, T![+], false>)>,
                T!['{'],
                RepeatMin<0, TraitItem>,
                T!['}'],
            )>()?;

        let generics = Generics {
            params: gen_params.map(|g| g.items_owned()).unwrap_or_default(),
            where_clause,
            span: stream.span_since(checkpoint),
        };

        Ok(Trait {
            name,
            generics,
            super_traits: super_traits
                .map(|(_colon, bounds)| bounds.value_owned())
                .unwrap_or_default(),
            items: items.value_owned(),
            span,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraitItem {
    /// Trait item
    pub item: TraitItemKind,
    pub attributes: Vec<Attribute>,
    // Trait items don't need visibility (they're always public in the trait's context)
    // pub visibility: Visibility,
    pub span: Span,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for TraitItem {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let checkpoint = stream.checkpoint();
        let (attributes, item, _semicolon) =
            stream.parse::<(AttributesList, TraitItemKind, Option<T![;]>)>()?;

        let span = stream.span_since(checkpoint);
        Ok(TraitItem {
            item,
            attributes: attributes.0,
            span,
        })
    }
}

/// Item within a trait
#[derive(Debug, Clone, PartialEq)]
pub enum TraitItemKind {
    /// Method declaration: `fn foo(&self) -> i32;`
    Method(Method),
    /// Associated type: `type Item;`
    AssociatedType(AssociatedType),
    /// Associated constant: `const MAX: i32;`
    Constant(AssociatedConst),
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for TraitItemKind {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let res = match_map!(
            stream,
            Method => Self::Method,
            AssociatedConst => Self::Constant,
            AssociatedType => Self::AssociatedType,
        )?;
        Ok(res)
    }
}
