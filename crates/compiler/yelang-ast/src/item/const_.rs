/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 11/12/2025
 */

use crate::T;
use crate::{Ident, Type, expr::Expr};
use yelang_lexer::{ParseTokenStream, TokenResult, TokenStream};

/// Constant declaration
///
/// # Example
/// ```
/// const PI: f64 = 3.14159;
/// pub const MAX_SIZE: usize = 1024;
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Const {
    pub name: Ident,
    pub ty: Type,
    pub value: Expr,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for Const {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let (_const, name, _colon, ty, _eq, value, _) =
            stream.parse::<(T![const], Ident, T![:], Type, T![=], Expr, T![;])>()?;

        Ok(Const { name, ty, value })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Interner;
    use crate::tokenizer::tokens::TokenKind;

    #[test]
    fn test_parse_const() {
        let input = "const PI: f64 = 3.14159;";
        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(input, &mut interner).unwrap();
        let const_def = stream.parse::<Const>().unwrap();

        assert_eq!(const_def.name.as_str(&interner), "PI");
        // Check that parsing succeeded
    }
}
