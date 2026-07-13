/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 11/12/2025
 */

use super::generics::GenericParamsParser;
use super::{Generics, WhereClause};
use crate::item::{Attribute, AttributesList};
use crate::{Ident, T, Type};
use yelang_lexer::{ParseTokenStream, SeparatedList, Span, TokenResult, TokenStream, match_map};

/// Struct definition
///
/// # Example
/// ```
/// struct Point {
///     x: i32,
///     y: i32,
/// }
///
/// struct Tuple(i32, i32);
///
/// struct Unit;
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Struct {
    /// Struct name
    pub name: Ident,
    /// Generic parameters
    pub generics: Generics,
    /// Struct fields (named, tuple, or unit)
    pub fields: StructFields,
    pub span: Span,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for Struct {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let checkpoint = stream.checkpoint();

        // struct Name <T> where ... { fields }
        type StructTuple = (
            T![struct],
            Ident,
            Option<GenericParamsParser>, // <T>
            Option<WhereClause>,         // where ...
            StructFields,
        );

        let (_struct, name, gen_params, where_clause, fields) = stream.parse::<StructTuple>()?;

        let generics = Generics {
            params: gen_params.map(|g| g.items_owned()).unwrap_or_default(),
            where_clause, // In structs, where clause is parsed right here
            span: stream.span_since(checkpoint),
        };

        Ok(Struct {
            name,
            generics,
            fields,
            span: stream.span_since(checkpoint),
        })
    }
}

/// Different struct field styles
#[derive(Debug, Clone, PartialEq)]
pub enum StructFields {
    /// Named fields: `{ x: i32, y: i32 }`
    ///
    /// # Example
    /// ```
    /// struct Point {
    ///     x: i32,
    ///     y: i32,
    /// }
    /// ```
    Named(Vec<FieldDef>),

    /// Tuple fields: `(i32, i32)`
    ///
    /// # Example
    /// ```
    /// struct Point(i32, i32);
    /// ```
    Tuple(Vec<Type>),

    /// Unit struct (no fields)
    ///
    /// # Example
    /// ```
    /// struct Unit;
    /// ```
    Unit,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for StructFields {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        match_map!(stream,
            (T!['{'], Option<SeparatedList<FieldDef, T![,], true>>, Option<T![,]>, T!['}']) => |(_l, fields, _trailing, _r)| StructFields::Named(fields.map(|f| f.value_owned()).unwrap_or_default()),
            (T!['('], Option<SeparatedList<Type, T![,], true>>, Option<T![,]>, T![')'], T![;]) => |(_l, types, _trailing, _r, _semi)| StructFields::Tuple(types.map(|t| t.value_owned()).unwrap_or_default()),
            T![;] => |_| StructFields::Unit
        )
    }
}

/// A field in a struct
///
/// # Example
/// ```
/// x: i32
/// name: string
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct FieldDef {
    /// Field attributes (decorators) like `@primary`, `@relation(...)`, ...
    pub attributes: Vec<Attribute>,
    /// Field name
    pub name: Ident,
    /// Field type
    pub ty: Type,
    pub span: Span,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for FieldDef {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let checkpoint = stream.checkpoint();
        let attributes = stream.parse::<AttributesList>()?.0;
        let (name, _colon, ty) = stream.parse::<(Ident, T![:], Type)>()?;
        let span = stream.span_since(checkpoint);
        Ok(FieldDef {
            attributes,
            name,
            ty,
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
    fn test_parse_named_struct() {
        let input = "struct Point { x: i32, y: i32 }";
        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(input, &mut interner).unwrap();
        let struct_def = stream.parse::<Struct>().unwrap();

        assert_eq!(struct_def.name.as_str(&interner), "Point");
        match &struct_def.fields {
            StructFields::Named(fields) => {
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0].name.as_str(&interner), "x");
                assert_eq!(fields[1].name.as_str(&interner), "y");
            }
            _ => panic!("Expected named fields"),
        }
    }

    #[test]
    fn test_parse_tuple_struct() {
        let input = "struct Tuple(i32, i32);";
        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(input, &mut interner).unwrap();
        let struct_def = stream.parse::<Struct>().unwrap();

        assert_eq!(struct_def.name.as_str(&interner), "Tuple");
        match &struct_def.fields {
            StructFields::Tuple(types) => {
                assert_eq!(types.len(), 2);
            }
            _ => panic!("Expected tuple fields"),
        }
    }

    #[test]
    fn test_parse_tuple_struct_with_semicolon() {
        let input = "struct Tuple(i32, i32);";
        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(input, &mut interner).unwrap();
        let struct_def = stream.parse::<Struct>().unwrap();

        assert_eq!(struct_def.name.as_str(&interner), "Tuple");
        match &struct_def.fields {
            StructFields::Tuple(types) => {
                assert_eq!(types.len(), 2);
            }
            _ => panic!("Expected tuple fields"),
        }

        assert!(stream.is_eof(), "expected to consume trailing ';'");
    }

    #[test]
    fn test_parse_unit_struct() {
        let input = "struct Unit;";
        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(input, &mut interner).unwrap();
        let struct_def = stream.parse::<Struct>().unwrap();

        assert_eq!(struct_def.name.as_str(&interner), "Unit");
        match &struct_def.fields {
            StructFields::Unit => {}
            _ => panic!("Expected unit struct"),
        }
    }
}
