/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 21/02/2025
 */
use super::Expr;
use crate::{Ident, T};
use yelang_lexer::{ArrayCreator, ParseTokenStream, Span, TokenResult, TokenStream, match_map};

#[derive(Debug, Clone, PartialEq)]
pub struct DocumentAccess {
    pub(crate) base: Box<Expr>,
    pub(crate) object: Document,
}

impl DocumentAccess {
    pub fn base(&self) -> &Expr {
        &self.base
    }

    pub fn object(&self) -> &Document {
        &self.object
    }

    pub fn fields(&self) -> &[DocumentField] {
        self.object.fields()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    pub fields: Vec<DocumentField>,
    pub span: Span,
}

impl Document {
    pub fn fields(&self) -> &[DocumentField] {
        &self.fields
    }

    pub fn span(&self) -> Span {
        self.span
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for Document {
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
            stream.parse_with_span::<ArrayCreator<T!['{'], DocumentField, T![,], T!['}']>>()?;

        Ok(Document {
            fields: obj.items_owned(),
            span,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct KeyVal {
    pub key: Ident,
    pub value: Expr,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for KeyVal {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let (key, _, val) = stream.parse::<(Ident, T![:], Expr)>()?;
        Ok(KeyVal { key, value: val })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct KeyOnly {
    pub key: Ident,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for KeyOnly {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let key = stream.parse::<Ident>()?;
        Ok(KeyOnly { key })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Spread {
    // pub op: T![...],
    pub expr: Expr,
}

// type SpreadOperator = (T![.], T![.], T![.]);

impl ParseTokenStream<crate::tokenizer::TokenKind> for Spread {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let (_, expr) = stream.parse::<(T![..], Expr)>()?;
        Ok(Spread { expr })
    }
}

/// A field inside an object projection.
#[derive(Debug, Clone, PartialEq)]
pub enum DocumentField {
    KeyVal(KeyVal),
    KeyOnly(KeyOnly),
    Spread(Spread),
}

impl DocumentField {
    // pub fn new(key: Ident, val: Expr) -> Self {
    //     Self { key, val }
    // }

    pub fn key(&self) -> Option<&Ident> {
        match self {
            DocumentField::KeyVal(kv) => Some(&kv.key),
            DocumentField::KeyOnly(ko) => Some(&ko.key),
            DocumentField::Spread { .. } => None,
        }
    }

    pub fn val(&self) -> Option<&Expr> {
        match self {
            DocumentField::KeyVal(kv) => Some(&kv.value),
            DocumentField::KeyOnly { .. } => None,
            DocumentField::Spread { .. } => None,
        }
    }

    pub fn spread(&self) -> Option<&Spread> {
        match self {
            DocumentField::Spread(s) => Some(s),
            _ => None,
        }
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for DocumentField {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let field = match_map!(
            stream,
            Spread => DocumentField::Spread,
            KeyVal => DocumentField::KeyVal,
            KeyOnly => DocumentField::KeyOnly,
        )?;

        Ok(field)
    }
}
