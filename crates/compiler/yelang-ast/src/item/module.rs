/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 11/12/2025
 */

use crate::{Ident, Item, T};
use yelang_lexer::{Either, ParseTokenStream, RepeatMin, SeparatedList, TokenResult, TokenStream};

/// Module definition
///
/// # Example
/// ```
/// mod utils { ... }
/// pub mod helpers { ... }
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ModDef {
    pub name: Ident,
    pub kind: ModKind,
}

impl ModDef {
    pub fn name(&self) -> &Ident {
        &self.name
    }

    pub fn kind(&self) -> &ModKind {
        &self.kind
    }

    pub fn is_inline(&self) -> bool {
        matches!(self.kind, ModKind::Inline { .. })
    }

    pub fn is_external(&self) -> bool {
        matches!(self.kind, ModKind::External)
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for ModDef {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let (_mod, name, mk) = stream.parse::<(T![mod], Ident, ModKind)>()?;

        Ok(Self { name, kind: mk })
    }
}

/// Module kind - inline vs external
#[derive(Debug, Clone, PartialEq)]
pub enum ModKind {
    /// Inline module with body: `mod name { ... }`
    Inline { items: Vec<Item> },
    /// External module: `mod name;`
    External,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for ModKind {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        type ModKind = Either<
            // External module
            T![;],
            // Inline module
            (T!['{'], RepeatMin<0, Item>, T!['}']),
        >;

        match stream.parse::<ModKind>()? {
            Either::Left(_semicolon) => Ok(Self::External),
            Either::Right((_lbrace, items, _rbrace)) => Ok(Self::Inline {
                items: items.value_owned(),
            }),
        }
    }
}
