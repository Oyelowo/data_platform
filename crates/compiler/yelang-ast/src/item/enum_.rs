/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 11/12/2025
 */

use super::generics::GenericParamsParser;
use super::{Attribute, AttributesList, FieldDef, Generics, WhereClause};
use crate::ExprKind;
use crate::Literal;
use crate::{Expr, Ident, T, Type};
use yelang_lexer::ArrayCreator;
use yelang_lexer::TokenError;
use yelang_lexer::match_map;
use yelang_lexer::{ParseTokenStream, SeparatedList, Span, TokenResult, TokenStream};

/// Enum definition
///
/// # Example
/// ```
/// enum Option<T> {
///     Some(T),
///     None,
/// }
///
/// enum Result<T, E> {
///     Ok(T),
///     Err(E),
/// }
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Enum {
    /// Enum name
    pub name: Ident,
    /// Generic parameters
    pub generics: Generics,
    /// Enum variants
    pub variants: Vec<VariantDef>,
}

/// A variant in an enum
///
/// # Example
/// ```
/// Some(T)          // Tuple variant
/// None             // Unit variant
/// Point { x, y }   // Struct variant
/// Value = 42       // With discriminant
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct VariantDef {
    /// Variant attributes/decorators
    pub attributes: Vec<Attribute>,
    /// Variant name
    pub name: Ident,
    /// Variant kind (unit, tuple, or struct)
    pub kind: VariantKind,
    /// Optional discriminant value
    pub discriminant: Option<Expr>,
    pub span: Span,
}

/// Kind of enum variant
#[derive(Debug, Clone, PartialEq)]
pub enum VariantKind {
    /// Unit variant (no data)
    ///
    /// # Example
    /// ```
    /// None
    /// Red
    /// ```
    Unit,

    /// Tuple variant with positional fields
    ///
    /// # Example
    /// ```
    /// Some(T)
    /// Point(i32, i32)
    /// ```
    Tuple(Vec<Type>),

    /// Struct variant with named fields
    ///
    /// # Example
    /// ```
    /// Point { x: i32, y: i32 }
    /// Error { message: string, code: i32 }
    /// ```
    Struct(Vec<FieldDef>),
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for VariantKind {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let res = match_map!(
            stream,
            (T!['('], SeparatedList<Type, T![,], true>, T![')'] ) => |tys| {
                VariantKind::Tuple(tys.1.value_owned())
            },
            (T!['{'], SeparatedList<FieldDef, T![,], true>, T!['}'] ) => |fields| {
                VariantKind::Struct(fields.1.value_owned())
            },
        );
        Ok(res.unwrap_or(VariantKind::Unit))
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for VariantDef {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let checkpoint = stream.checkpoint();
        let ((attrs, name, kind), span) =
            stream.parse_with_span::<(AttributesList, Ident, VariantKind)>()?;
        let AttributesList(attributes) = attrs;
        // Optional discriminant: = <expr>
        let discriminant = stream
            .parse::<Option<(T![=], Expr)>>()?
            .map(|(_eq, expr)| expr);

        let is_lit = discriminant
            .as_ref()
            .map(|expr| matches!(expr.kind, ExprKind::Literal(Literal::Int(_))))
            .unwrap_or(false);

        // TODO: consider if this should be done during semantic analysis instead
        if !is_lit && discriminant.is_some() {
            return Err(TokenError::CustomError {
                msg: "Enum variant discriminant must be an integer literal".to_string(),
                span,
            });
        }

        let span = stream.span_since(checkpoint);
        Ok(VariantDef {
            attributes,
            name,
            kind,
            discriminant,
            span,
        })
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for Enum {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let checkpoint = stream.checkpoint();

        let (_enum, name, gen_params, where_clause, array_cr) = stream.parse::<(
            T![enum],
            Ident,
            Option<GenericParamsParser>,
            Option<WhereClause>,
            ArrayCreator<T!['{'], VariantDef, T![,], T!['}']>,
        )>()?;

        let generics = Generics {
            params: gen_params.map(|g| g.items_owned()).unwrap_or_default(),
            where_clause,
            span: stream.span_since(checkpoint),
        };

        Ok(Enum {
            name,
            generics,
            variants: array_cr.items_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Interner;
    use crate::tokenizer::tokens::TokenKind;

    #[test]
    fn test_parse_simple_enum() {
        let input = "enum Option { Some, None }";
        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(input, &mut interner).unwrap();
        let enum_def = stream.parse::<Enum>().unwrap();

        assert_eq!(enum_def.name.as_str(&interner), "Option");
        assert_eq!(enum_def.variants.len(), 2);
        assert_eq!(enum_def.variants[0].name.as_str(&interner), "Some");
        assert_eq!(enum_def.variants[1].name.as_str(&interner), "None");
    }

    #[test]
    fn test_parse_tuple_enum() {
        let input = "enum Result { Ok(i32), Err(string) }";
        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(input, &mut interner).unwrap();
        let enum_def = stream.parse::<Enum>().unwrap();

        assert_eq!(enum_def.name.as_str(&interner), "Result");
        assert_eq!(enum_def.variants.len(), 2);
        match &enum_def.variants[0].kind {
            VariantKind::Tuple(types) => assert_eq!(types.len(), 1),
            _ => panic!("Expected tuple variant"),
        }
    }
}
