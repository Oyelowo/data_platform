/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 11/12/2025
 */

use super::generics::GenericParamsParser;
use super::*;
use crate::{Ident, Path, T, Type};
use yelang_lexer::{ParseTokenStream, RepeatMin, Span, TokenError, TokenResult, TokenStream};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Defaultness {
    /// This item is explicitly marked `default` and may be specialized.
    Default,
    /// This item is not marked `default`.
    Final,
}

/// Implementation block
///
/// # Example
/// ```
/// impl Point {
///     fn new(x: i32, y: i32) -> Self { ... }
/// }
///
/// impl Display for Point {
///     fn fmt(&self) -> string { ... }
/// }
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Impl {
    pub attributes: Vec<Attribute>,
    pub defaultness: Defaultness,
    /// Generic parameters
    pub generics: Generics,
    /// Optional trait being implemented (for trait impls)
    pub trait_impl: Option<Path>,
    /// True for negative impls: `impl !Trait for Type {}`
    pub is_negative: bool,
    /// Type being implemented for
    pub self_ty: Type,
    /// Implementation items
    pub items: Vec<ImplItem>,
    pub span: Span,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for Impl {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let checkpoint = stream.checkpoint();
        let (attributes, default_kw, _, gen_params, trait_impl, ty, where_clause, _, items, _) =
            stream.parse::<(
                AttributesList,
                Option<T![default]>,
                T![impl],
                Option<GenericParamsParser>,
                Option<(Option<T![!]>, Path, T![for])>,
                Type,
                Option<WhereClause>,
                T!['{'],
                RepeatMin<0, ImplItem>,
                T!['}'],
            )>()?;

        let is_negative = trait_impl
            .as_ref()
            .and_then(|(bang, _p, _)| bang.as_ref())
            .is_some();
        let defaultness = if default_kw.is_some() {
            Defaultness::Default
        } else {
            Defaultness::Final
        };

        let items = items.value_owned();

        if is_negative && !items.is_empty() {
            return Err(TokenError::CustomError {
                msg: "negative impl blocks must be empty".to_string(),
                span: stream.span_since(checkpoint),
            });
        }

        if matches!(defaultness, Defaultness::Default) && trait_impl.is_none() {
            return Err(TokenError::CustomError {
                msg: "default impl is only allowed for trait impls".to_string(),
                span: stream.span_since(checkpoint),
            });
        }

        if matches!(defaultness, Defaultness::Default) && is_negative {
            return Err(TokenError::CustomError {
                msg: "negative impls cannot be default impls".to_string(),
                span: stream.span_since(checkpoint),
            });
        }

        let generics = Generics {
            params: gen_params.map(|g| g.items_owned()).unwrap_or_default(),
            where_clause,
            span: stream.span_since(checkpoint),
        };

        Ok(Impl {
            attributes: attributes.0,
            defaultness,
            generics,
            is_negative,
            trait_impl: trait_impl.map(|(_bang, path, _)| path),
            self_ty: ty,
            items,
            span: stream.span_since(checkpoint),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImplItem {
    /// Implementation item
    pub item: ImplItemKind,
    pub defaultness: Defaultness,
    pub attributes: Vec<Attribute>,
    pub visibility: Visibility, // Placeholder for visibility
    pub span: Span,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for ImplItem {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let (item_data, span) = stream.parse_with_span::<(
            AttributesList,
            Visibility,
            Option<T![default]>,
            ImplItemKind,
        )>()?;
        let (AttributesList(attributes), visibility, default_kw, item) = item_data;
        Ok(ImplItem {
            item,
            defaultness: if default_kw.is_some() {
                Defaultness::Default
            } else {
                Defaultness::Final
            },
            attributes,
            visibility,
            span,
        })
    }
}

/// Item within an implementation block
#[derive(Debug, Clone, PartialEq)]
pub enum ImplItemKind {
    /// Method implementation
    Method(FnDef),
    /// Associated type binding
    AssociatedType(AssociatedTypeBinding),
    /// Associated constant
    Constant(AssociatedConst),
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for ImplItemKind {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        use yelang_lexer::match_map;
        match_map!(
            stream,
            FnDef => ImplItemKind::Method,
            AssociatedTypeBinding => ImplItemKind::AssociatedType,
            AssociatedConst => ImplItemKind::Constant,
        )
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for AssociatedTypeBinding {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let checkpoint = stream.checkpoint();

        // Impl associated type bindings follow the same where-clause placement as type aliases.
        // Example: `type Item<T> = T where T: Bound;`
        let (_type, name, gen_params, _eq, ty, where_clause, _semi) = stream.parse::<(
            T![type],
            Ident,
            Option<GenericParamsParser>,
            T![=],
            Type,
            Option<WhereClause>,
            T![;],
        )>()?;

        let generics = Generics {
            params: gen_params.map(|g| g.items_owned()).unwrap_or_default(),
            where_clause,
            span: stream.span_since(checkpoint),
        };

        let span = stream.span_since(checkpoint);
        Ok(AssociatedTypeBinding {
            name,
            generics,
            ty,
            span,
        })
    }
}
