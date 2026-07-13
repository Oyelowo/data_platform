/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2025 Oyelowo Oyedayo
 * Date 08/03/2025
 */

use super::{CreatePath, CreationData};
use crate::{Expr, Ident, T, TokenKind, Type};
use yelang_lexer::{ParseTokenStream, SeparatedList, Span, TokenError, TokenResult, TokenStream};

#[derive(Debug, Clone, PartialEq)]
pub struct ConflictClause {
    pub fields: Vec<Ident>,
    pub action: ConflictAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictAction {
    Replace,
    Merge,
    Ignore,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UpsertQ {
    pub var: Ident,
    pub binding: Ident,
    pub table: Type,
    pub data: CreationData,
    pub on_conflict: Option<ConflictClause>,
    pub links: Vec<CreatePath>,
    pub return_: Option<Expr>,
    pub span: Span,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for UpsertQ {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let checkpoint = stream.checkpoint();
        stream.parse::<T![upsert]>()?;

        let is_block = matches!(stream.peek().map(|t| t.kind()), Some(TokenKind::OpenBrace));

        type UpsertHeader = (Ident, T![@], Ident, T![:], Type, CreationData);

        let (var, _at, binding, _, table, data, on_conflict, links, return_expr) = if is_block {
            stream.parse::<T!['{']>()?;
            let (var, at, binding, col, table, data) = stream.parse::<UpsertHeader>()?;
            let on_conflict = parse_conflict_clause(stream)?;
            let links =
                stream.parse::<Option<(T![link], SeparatedList<CreatePath, T![,], true>)>>()?;
            let tail = stream
                .parse::<Option<(T![;], Expr)>>()?
                .map(|(_, expr)| expr);
            stream.parse::<T!['}']>()?;

            (var, at, binding, col, table, data, on_conflict, links, tail)
        } else {
            let (var, at, binding, col, table, data) = stream.parse::<UpsertHeader>()?;
            let on_conflict = parse_conflict_clause(stream)?;
            let links =
                stream.parse::<Option<(T![link], SeparatedList<CreatePath, T![,], true>)>>()?;
            (var, at, binding, col, table, data, on_conflict, links, None)
        };

        Ok(Self {
            var,
            binding,
            table,
            data,
            on_conflict,
            links: links
                .map(|(_, links)| links.value_owned())
                .unwrap_or_default(),
            return_: return_expr,
            span: stream.span_since(checkpoint),
        })
    }
}

impl UpsertQ {
    pub fn span(&self) -> Span {
        self.span
    }
}

fn parse_conflict_clause(
    stream: &mut TokenStream<crate::tokenizer::TokenKind>,
) -> TokenResult<Option<ConflictClause>> {
    let checkpoint = stream.checkpoint();
    let Some(token) = stream.peek() else {
        stream.restore(checkpoint);
        return Ok(None);
    };
    if !matches!(token.kind(), TokenKind::On) {
        stream.restore(checkpoint);
        return Ok(None);
    }
    stream.advance();

    let conflict = stream.parse::<Ident>()?;
    if conflict.as_str(stream.interner()) != "conflict" {
        stream.restore(checkpoint);
        return Ok(None);
    }

    stream.parse::<T!['(']>()?;
    let fields = stream
        .parse::<SeparatedList<Ident, T![,], true>>()?
        .value_owned();
    stream.parse::<T![')']>()?;

    if fields.is_empty() {
        return Err(TokenError::CustomError {
            msg: "UPSERT conflict clauses require at least one conflict field".into(),
            span: stream.span_since(checkpoint),
        });
    }

    let action_ident = stream.parse::<Ident>()?;
    let action = match action_ident.as_str(stream.interner()) {
        "replace" => ConflictAction::Replace,
        "merge" => ConflictAction::Merge,
        "ignore" => ConflictAction::Ignore,
        other => {
            return Err(TokenError::CustomError {
                msg: format!(
                    "UPSERT conflict clauses support only `replace`, `merge`, or `ignore`; found `{other}`"
                ),
                span: action_ident.span,
            });
        }
    };

    Ok(Some(ConflictClause { fields, action }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokenizer::TokenKind;
    use yelang_lexer::{All, TokenizeChars};

    #[test]
    fn test_upsert_statement_parses_conflict_clause() {
        use crate::Interner;

        let input = "upsert user@u:User { id: 1 } on conflict (id, email) merge";
        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(input, &mut interner).unwrap();
        let parsed = UpsertQ::parse(&mut stream).expect("expected upsert parse success");

        let conflict = parsed
            .on_conflict
            .expect("expected parsed UPSERT conflict clause");
        assert_eq!(conflict.fields.len(), 2);
        assert_eq!(conflict.fields[0].as_str(&interner), "id");
        assert_eq!(conflict.fields[1].as_str(&interner), "email");
        assert_eq!(conflict.action, ConflictAction::Merge);
    }

    #[test]
    fn test_upsert_statement() {
        let input = "upsert user:User { 
                id: 'User:1', 
                name: 'John Doe', 
                age: 30, 
                email: 'oyelowo.oss@gmail.com'
            };";

        let input = "upsert user:User { 
                id: 'User:1', 
                name: 'John Doe', 
                age: 30, 
                email: 'oyelowo.oss@gmail.com'
            }
            return {
                id,
                name: concat(user.name, 4)
            };";

        let input = "upsert user:User { 
                id: 'User:1', 
                name: 'John Doe', 
                age: 30, 
                email: 'oyelowo.oss@gmail.com'
            }
            return user.{
                id,
                name: concat(user.name, 4)
            };";

        let input = "upsert user:User [{ 
                id: 'User:1', 
                name: 'John Doe', 
                age: 30, 
                email: 'oyelowo.oss@gmail.com'
            }]
            return {
                id,
                name: concat(user.name, 4)
            };";

        // let input = "upsert user:User Value [{
        let input = "
            upsert user:User [{ 
                id: 'User:1', 
                name: 'John Doe', 
                age: 30, 
                email: 'oyelowo.oss@gmail.com'
            }]

            link (user) -> [knows:UserKnowsUser {}] -> (friend:User where friend.name == 'John')

            return user[where user.age > 20].{
                id,
                name: concat(user.name, 4)
            }
        ";

        // let mut stream = TokenKind::tokenize(input).unwrap();
        // let stmt = stream.parse::<All<Upsert>>().unwrap().into_inner();

        // panic!("{:#?}", stmt);
        // assert_eq!(stmt.label, Some(Ident::new_unchecked("user")));
    }
}

// UPSERT user:User:123 {
//   name: "John Doe",
//   age: 35,
//   last_login: time::now()
// };
//
// UPSERT user:User[*] [
//   { id: 1, name: "Alice", age: 25 },
//   { id: 2, name: "Bob", age: 30 }
// ];
//
//
