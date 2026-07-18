/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2025 Oyelowo Oyedayo
 * Date 08/03/2025
 */

use super::{CreatePath, parse_query_tail};
use crate::{Array, Expr, Ident, Object, T, TokenKind, Type};
use yelang_lexer::{ParseTokenStream, SeparatedList, Span, TokenResult, TokenStream, match_map};

// create user:User { name: 'John', age: 30 };
// create user:User [{ name: 'John', age: 30 }];
#[derive(Debug, Clone, PartialEq)]
pub struct CreateQ {
    pub var: Ident,
    pub binding: Ident,
    pub table: Type,
    pub data: CreationData,
    pub links: Vec<CreatePath>,
    pub return_: Option<Expr>,
    pub span: Span,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for CreateQ {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let checkpoint = stream.checkpoint();

        stream.parse::<T![create]>()?;

        // New block form (keyword must be immediately followed by `{`):
        //   create { <insert-body> ; <expr> }
        // Tail expression is the query value.
        let is_block = matches!(stream.peek().map(|t| t.kind()), Some(TokenKind::OpenBrace));

        let (var, _at, binding, _col, table, data, links, return_expr) = if is_block {
            stream.parse::<T!['{']>()?;
            let (var, at, binding, col, table, data, links) = stream.parse::<InsertBody>()?;

            let tail = parse_query_tail(stream)?;

            stream.parse::<T!['}']>()?;

            (var, at, binding, col, table, data, links, tail)
        } else {
            let (var, at, binding, col, table, data, links) = stream.parse::<InsertBody>()?;
            (var, at, binding, col, table, data, links, None)
        };

        Ok(Self {
            var,
            binding,
            table,
            data,
            links: links
                .map(|(_, links)| links.value_owned())
                .unwrap_or_default(),
            return_: return_expr,
            span: stream.span_since(checkpoint),
        })
    }
}

// type Links = Option<(T![link], SeparatedList<CreatePath, T![,]>)>;
// Helper type to keep the return signature clean
// users@u:User { ... } link ... return ... { ... }
pub(crate) type InsertBody = (
    Ident, // var
    T![@], // @
    Ident, // binding
    T![:],
    Type,                                                       // table
    CreationData,                                               // data
    Option<(T![link], SeparatedList<CreatePath, T![,], true>)>, // links
);

#[derive(Debug, Clone, PartialEq)]
pub enum CreationData {
    Object(Object),
    Array(Array),
}
impl ParseTokenStream<crate::tokenizer::TokenKind> for CreationData {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        match_map!(
            stream,
            Object => CreationData::Object,
            Array => CreationData::Array
        )
    }
}

impl CreateQ {
    pub fn span(&self) -> Span {
        self.span
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Stmt, TokenKind};
    use yelang_lexer::{All, TokenStream, TokenizeChars};
    // create user:User {
    //   id: uuid(),
    //   name: 'John Doe',
    //   age: 30,
    //   email: "john@example.com"
    // };

    // create user:User {
    //   id: uuid(),
    //   name: 'John Doe',
    //   age: 30,
    //   email: 'john@example.com'
    // }
    // return {
    //  id,
    //  name, concat(user.name, 4)
    // };

    // create user:User[*] [
    //   { id: uuid(), name: 'Alice', age: 25 },
    //   { id: uuid(), name: 'Bob', age: 28 }
    // ];
    //

    // create user:User {
    //   id: uuid(),
    //   name: 'Jane Doe',
    //   age: 27,
    //   contacts: [
    //     { type: 'email', value: 'jane@example.com' },
    //     { type: 'phone', value: '+1234567890' }
    //   ]
    // };

    #[test]
    fn test_create_statement() {
        let input = "create user:User { 
                id: 'User:1', 
                name: 'John Doe', 
                age: 30, 
                email: 'oyelowo.oss@gmail.com'
            };";

        let input = "create user:User { 
                id: 'User:1', 
                name: 'John Doe', 
                age: 30, 
                email: 'oyelowo.oss@gmail.com'
            }
            return {
                id,
                name: concat(user.name, 4)
            };";

        // TODO: Consider this syntax over the others
        // let _syntax_to_consider = "
        //     let user = create User {
        //         id: 'User:1',
        //         name: 'John Doe',
        //         age: 30,
        //         email: 'oyelowo.oss@gmail.com'
        //     };
        //
        //     return {
        //         id: user.id,
        //         name: concat(user.name, 4)
        //     };";

        let input = "create user:User { 
                id: 'User:1', 
                name: 'John Doe', 
                age: 30, 
                email: 'oyelowo.oss@gmail.com'
            }
            return user.{
                id,
                name: concat(user.name, 4)
            };";

        let input = "create user:User [{ 
                id: 'User:1', 
                name: 'John Doe', 
                age: 30, 
                email: 'oyelowo.oss@gmail.com'
            }]
            return {
                id,
                name: concat(user.name, 4)
            };";

        // link (user) -> [knows:UserKnowsUser {}] -> (friend:User WHERE friend.name == 'John')
        let input = "create user:User [
                { 
                    id: 'User:1', 
                    name: 'Oyelowo Oyedayo', 
                    age: 123, 
                    email: 'oyelowo.oss@gmail.com'
                },
                { 
                    id: 'User:2', 
                    name: 'Ann', 
                    age: 112, 
                    email: 'google@gmail.com'
                }

            ]

            link (users@u) -> [follow:UserFollowsUser {
                    since: now(),
                    mutual: false,
                    weight: u.age * len(u.name)
                }] -> (targes:User),

                (users) -> [eat:UserEatsFood {
                    flavor: 'sweet',
                    time: dt'2020-01-01'
                }] -> (foods:Food),

                (users@u where u.age > 40) -> [eat:UserLikesBook] -> (books@b:Book where bbook.title == 'The Alchemist')


            return user@u[where u.age > 20].{
                id,
                name: concat(u.name, 4)
            }
        ";

        // let mut stream = TokenKind::tokenize(input).unwrap();
        // let stmt = stream.parse::<All<CreateStatement>>();
        // panic!("{:#?}", stmt);
        // stream.reset_dangerous();
        // create stmt is an expr and shouldnt have a semicolon by itself
        // let stmt = stream.parse::<All<CreateStatement>>();
        // panic!("{:#?}", stmt);

        // stream.reset_dangerous();
        // let stmt = stream.parse::<All<Statement>>().unwrap();

        // panic!("{:#?}", stmt);
        // assert_eq!(stmt.label, Some(Ident::new_unchecked("user")));
    }

    // #[test]
    // fn test_deeply_nested_arrays() {
    //     let query = "create user:User [[[[[{ name: 'deep' }]]]]];";
    //     let mut stream = Token::tokenize(query).unwrap();
    //     let stmt = stream.parse::<CreateStatement>().unwrap();
    //     assert!(stmt.data_is_deep_nested());
    // }
}
