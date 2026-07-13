/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 15/11/2025
 */

use crate::{Ident, Type, expr::Expr};
use yelang_lexer::{ParseTokenStream, TokenResult, TokenStream};

// FIXME: Probably wont suport this
/// Static declaration
///
/// # Example
/// ```
/// static mut COUNTER: u32 = 0;
/// pub static PI: f64 = 3.14159;
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Static {
    pub name: Ident,
    pub ty: Type,
    pub value: Expr,
    pub mutability: bool, // true for `static mut`
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for Static {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        use crate::T;

        let (_st, muta, name, _colon, ty, _eq, value, _semicolon) = stream.parse::<(
            T![static],
            Option<T![mut]>,
            //
            Ident,
            T![:],
            Type,
            T![=],
            Expr,
            T![;],
        )>()?;

        Ok(Static {
            name,
            ty,
            value,
            mutability: muta.is_some(),
        })
    }
}
