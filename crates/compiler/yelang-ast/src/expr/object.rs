use crate::T;
/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 09/02/2025
 */
use crate::{Expr, Ident};
use yelang_lexer::{ArrayCreator, ParseTokenStream, Span, TokenResult, TokenStream, match_map};

#[derive(Debug, Clone, PartialEq)]
pub struct Object {
    pub fields: Vec<ObjectField>,
    pub span: Span,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for Object {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        // // IMPERATIVE STYLE
        // let start = stream.checkpoint();
        // stream.consume(Token::OpenBrace)?;
        // let mut fields = Vec::new();
        // while !stream.is_eof() {
        //     // we dont wnna consume the close brace since we do that outside
        //     if stream.verify(Token::CloseBrace).is_ok() {
        //         break;
        //     }
        //
        //     let key = stream.parse::<K>()?;
        //
        //     stream.consume(Token::Colon)?;
        //
        //     let value = stream.parse::<V>()?;
        //     fields.push((key, value));
        //
        //     if stream.consume(Token::Comma).is_err() {
        //         break;
        //     }
        // }
        // stream.span_since(checkpoint);

        // DECLARATIVE STYLE
        let (obj, span) =
            stream.parse_with_span::<ArrayCreator<T!['{'], ObjectField, T![,], T!['}']>>()?;

        Ok(Object {
            fields: obj.items_owned(),
            span,
        })
    }
}

impl Object {
    pub fn fields(&self) -> &[ObjectField] {
        &self.fields
    }

    pub fn span(&self) -> Span {
        self.span
    }
}

/// A field inside an object projection.
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectField {
    pub(crate) key: Ident,
    pub(crate) val: Expr,
}

impl ObjectField {
    pub fn new(key: Ident, val: Expr) -> Self {
        Self { key, val }
    }

    pub fn key(&self) -> &Ident {
        &self.key
    }

    pub fn value(&self) -> &Expr {
        &self.val
    }
}

// TODO: Support spread operator
// pub enum ObjectField {
//     KeyVal(Ident, T![:], Expr),
//     KeyOnly(Ident),
//     Spread(T![...], Expr),
// }

impl ParseTokenStream<crate::tokenizer::TokenKind> for ObjectField {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        // let span = stream.checkpoint();
        // let fields = stream.parse::<Either<KeyVal, KeyOnly>>()?;
        let keyval = match_map!(
            stream,
            (Ident, T![:], Expr) => |(key, _, val)| ObjectField {
                key,
                val,
            }
            // NOTE:: key val required for object. only object selectable can have key only
            // allow shorthand for key-value pairs
            // Ident => |key| ObjectField {
            //     key,
            //     val: Expr::Ident(key),
            // }
        )?;
        // let span = stream.span_since(span);

        Ok(keyval)
    }
}

// type KeyVal<K, V> = (K, T![":"], V);
// type Fields<K, V> = (
//     Repeat<(KeyVal<K, V>, T![","])>,
//     // Trailing
//     Option<(KeyVal<K, V>, Option<T![","]>)>,
// );
// impl<'a, K, V> ParseTokenStream<Token> for Object<K, V>
// where
//     K: ParseTokenStream<Token>,
//     V: ParseTokenStream<Token>,
// {
//     fn parse(stream: &mut TokenStream<Token>) -> TokenResult<Self> {
//         let start = stream.checkpoint();
//         stream.consume(Token::OpenBrace)?;
//         // let mut fields = Vec::new();
//         // while !stream.is_eof() {
//         //     // we dont wnna consume the close brace since we do that outside
//         //     if stream.verify(Token::CloseBrace).is_ok() {
//         //         break;
//         //     }
//         //
//         //     let key = stream.parse::<K>()?;
//         //
//         //     stream.consume(Token::Colon)?;
//         //
//         //     let value = stream.parse::<V>()?;
//         //     fields.push((key, value));
//         //
//         //     if stream.consume(Token::Comma).is_err() {
//         //         break;
//         //     }
//         // }
//
//         // Using the Repeat combinator
//         let fields_parsed = stream.parse::<Fields<K, V>>()?;
//         let mut fields = fields_parsed
//             .0
//             .value_owned()
//             .into_iter()
//             .map(|((k, _, v), _)| (k, v))
//             .collect::<Vec<(K, V)>>();
//
//         let trailing = fields_parsed.1.map(|((k, _, v), _)| (k, v));
//
//         if let Some((k, v)) = trailing {
//             fields.push((k, v));
//         }
//
//         stream.consume(Token::CloseBrace)?;
//
//         let span = stream.span_since(start);
//         Ok(Object { fields, span })
//     }
// }
