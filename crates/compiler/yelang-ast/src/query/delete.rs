/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 31/12/2024
 */

use crate::{Expr, Ident, T, TokenKind, Type};
use yelang_lexer::{ParseTokenStream, Span, TokenResult, TokenStream, match_map_res};

#[derive(Debug, Clone, PartialEq)]
pub struct DeleteQ {
    /// Collection label (e.g. `users` in `delete users@u:User ...`).
    pub var: Ident,
    /// Per-row binder (e.g. `u` in `delete users@u:User ...`).
    pub binding: Ident,
    pub table: Type,
    pub condition: Option<Expr>,
    pub return_: Option<Expr>,
    pub span: Span,
}

impl DeleteQ {}

impl ParseTokenStream<crate::tokenizer::TokenKind> for DeleteQ {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let checkpoint = stream.checkpoint();
        stream.parse::<T![delete]>()?;

        let is_block = matches!(stream.peek().map(|t| t.kind()), Some(TokenKind::OpenBrace));
        if is_block {
            stream.parse::<T!['{']>()?;
        }

        // Locked-in header: DELETE is always collection-driven.
        //
        // Syntax: `delete users@u:User ...`
        //
        // Rationale: `@u` is a per-element binder and must always be introduced by an
        // explicit collection label.
        type Header = (Ident, T![@], Ident, T![:], Type);
        let (var, _at, binding, _colon, table) = match_map_res!(
            stream,
            Header => |h| Ok(h)
        )?;

        let cond = stream
            .parse::<Option<(T![where], Expr)>>()?
            .map(|(_, expr)| expr);

        let tail = if is_block {
            stream
                .parse::<Option<(T![;], Expr)>>()?
                .map(|(_, expr)| expr)
        } else {
            None
        };

        if is_block {
            stream.parse::<T!['}']>()?;
        }

        let ret = tail;

        Ok(Self {
            var,
            binding,
            table,
            condition: cond,
            return_: ret,
            span: stream.span_since(checkpoint),
        })
    }
}

#[cfg(test)]
mod tests {
    // use super::*;
    // use crate::ast::tokenizer::TokenKind;
    // use crate::lexer::TokenizeChars;
    #[test]
    fn test_create_statement() {
        // let input = "
        //     delete users@u:User
        //     where user.age > 5
        //     return user[*].{
        //         id,
        //         name: concat(user.name, 4)
        //     };
        // ";
        // DELETE users@u:User:123;
        //
        // DELETE users@u:User[WHERE u.age < 18];
        //
        // DELETE users@u:User:123.contacts[WHERE contacts.type == "phone"];

        // let mut stream = TokenKind::tokenize(input).unwrap();
        // let stmt = stream.parse::<DeleteStatement>().unwrap();

        // panic!("{:#?}", stmt);
        // assert_eq!(stmt.label, Some(Ident::new_unchecked("user")));
    }
}
