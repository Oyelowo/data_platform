/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 11/12/2025
 */

use crate::{
    Ident, T, TraitBound,
    expr::Expr,
    item::{Generics, WhereClause, generics::GenericParamsParser},
    types::Type,
};
use yelang_lexer::{
    Either, ParseTokenStream, SeparatedList, Span, TokenResult, TokenStream, Verify,
};

/// Associated type in a trait
///
/// # Example
/// ```
/// type Item;
/// type Item: Clone;
/// type Item = i32;
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct AssociatedType {
    /// Type name
    pub name: Ident,
    /// Generic parameters for generic associated types (GATs)
    pub generics: Generics,
    /// Trait bounds on the associated type
    pub bounds: Vec<TraitBound>,
    /// Optional default type
    pub default: Option<Type>,
    pub span: Span,
}

/// Associated constant in a trait
///
/// # Example
/// ```
/// const MAX: i32;
/// const MAX: i32 = 100;
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct AssociatedConst {
    /// Constant name
    pub name: Ident,
    /// Constant type
    pub ty: Type,
    /// Optional default value (for trait definition)
    pub value: Option<Expr>,
    pub span: Span,
}

/// Associated type binding in an impl
///
/// # Example
/// ```
/// type Item = i32;
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct AssociatedTypeBinding {
    /// Type name
    pub name: Ident,
    /// Generic parameters for GAT bindings in impls
    pub generics: Generics,
    /// Bound type
    pub ty: Type,
    pub span: Span,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for AssociatedType {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        type TraitBoundPart = (T![:], SeparatedList<TraitBound, T![+], false>);
        type AssocTuple = (
            T![type],
            Ident,
            Option<GenericParamsParser>,
            Option<Either<(T![=], Type), TraitBoundPart>>,
            Option<WhereClause>,
            T![;],
        );

        let ((_, name, gen_params, target, where_clause, _semi), span) =
            stream.parse_with_span::<AssocTuple>()?;

        let (bounds, default) = match target {
            Some(Either::Left((_, ty))) => (vec![], Some(ty)),
            Some(Either::Right((_, bounds_list))) => (bounds_list.items(), None),
            None => (vec![], None),
        };

        let generics = Generics {
            params: gen_params.map(|g| g.items_owned()).unwrap_or_default(),
            where_clause,
            span,
        };

        Ok(AssociatedType {
            name,
            generics,
            bounds,
            default,
            span,
        })
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for AssociatedConst {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let ((_const, name, _colon, ty, value_option, _semicolon), span) = stream
            .parse_with_span::<(T![const], Ident, T![:], Type, Option<(T![=], Expr)>, T![;])>()?;

        Ok(AssociatedConst {
            name,
            ty,
            value: value_option.map(|(_, expr)| expr),
            span,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Interner;
    use crate::tokenizer::tokens::TokenKind;

    #[test]
    fn parse_associated_type_with_gat_params() {
        let src = "type Item<T>;";
        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(src, &mut interner).expect("tokenize");
        let assoc = stream.parse::<AssociatedType>().expect("parse");

        assert_eq!(assoc.name.as_str(&interner), "Item");
        assert_eq!(assoc.generics.params.len(), 1);
        assert!(assoc.generics.where_clause.is_none());
        assert!(assoc.bounds.is_empty());
        assert!(assoc.default.is_none());
        assert!(stream.is_eof());
    }

    #[test]
    fn parse_associated_type_with_trailing_where_clause() {
        let src = "type Item<T> where T: Bound;";
        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(src, &mut interner).expect("tokenize");
        let assoc = stream.parse::<AssociatedType>().expect("parse");

        assert_eq!(assoc.name.as_str(&interner), "Item");
        assert_eq!(assoc.generics.params.len(), 1);
        assert!(assoc.generics.where_clause.is_some());
        assert!(assoc.bounds.is_empty());
        assert!(assoc.default.is_none());
        assert!(stream.is_eof());
    }
}
