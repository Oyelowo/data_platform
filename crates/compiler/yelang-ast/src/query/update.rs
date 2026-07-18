/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 31/12/2024
 */

use super::{CreatePath, parse_query_tail};
use crate::{Expr, Ident, Object, T, Type, expr::AssignOpKind, tokenizer::TokenKind};
use yelang_lexer::{
    ParseTokenStream, SeparatedList, Span, TokenError, TokenResult, TokenStream, match_map_res,
};

#[derive(Debug, Clone, PartialEq)]
pub struct UpdateQ {
    pub var: Ident,
    pub binding: Ident,
    pub table: Type,
    pub mutation: UpdateMutation,
    pub links: Vec<CreatePath>,
    pub condition: Option<Expr>,
    pub return_: Option<Expr>,
    pub span: Span,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for UpdateQ {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let checkpoint = stream.checkpoint();
        stream.parse::<T![update]>()?;

        let is_block = matches!(stream.peek().map(|t| t.kind()), Some(TokenKind::OpenBrace));
        if is_block {
            stream.parse::<T!['{']>()?;
        }

        // Locked-in header: UPDATE is always collection-driven.
        //
        // Syntax: `update users@u:User ...`
        //
        // Rationale: `@u` is a per-element binder and must always be introduced by an
        // explicit collection label.
        type Header = (Ident, T![@], Ident, T![:], Type);
        let (var, _at, binding, _colon, table) = match_map_res!(
            stream,
            Header => |h| Ok(h)
        )?;

        // Mutation Logic: Check for SET keyword
        let mutation = if stream.parse::<T![set]>().is_ok() {
            // SET can be followed by braces or a single setter
            let setters = if stream.parse::<T!['{']>().is_ok() {
                // Parse SET { ... } (multiple setters with braces)
                let setters = stream
                    .parse::<SeparatedList<Setter, T![;], false>>()?
                    .value_owned();
                stream.parse::<Option<T![;]>>()?; // Optional trailing semicolon
                stream.parse::<T!['}']>()?;
                setters
            } else {
                // Parse SET field = value (single setter without braces)
                vec![stream.parse::<Setter>()?]
            };
            UpdateMutation::Set(setters)
        } else {
            // Parse CONTENT { ... } or merge object
            let obj = stream.parse::<Object>()?;
            UpdateMutation::Merge(obj)
        };

        // Optional Links
        let links = stream
            .parse::<Option<(T![link], SeparatedList<CreatePath, T![,], true>)>>()?
            .map(|(_, links)| links.value_owned())
            .unwrap_or_default();

        // Where Clause
        let condition = stream
            .parse::<Option<(T![where], Expr)>>()?
            .map(|(_, expr)| expr);

        let tail = if is_block { parse_query_tail(stream)? } else { None };

        if is_block {
            stream.parse::<T!['}']>()?;
        }

        let return_ = tail;

        Ok(Self {
            var,
            binding,
            table,
            mutation,
            links,
            condition,
            return_,
            span: stream.span_since(checkpoint),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum UpdateMutation {
    Merge(Object),
    Set(Vec<Setter>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Setter {
    pub path: Expr,
    pub op: SetterOp,
    pub value: Expr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetterOp {
    Assign,
    Increment,
    Decrement,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for Setter {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        // Parse the path (left side of =) allowing postfix access (e.g. `u.name`, `u.arr[0]`) but
        // disallowing infix operators (especially assignment) by starting at call precedence.
        use crate::expr::{Precedence, Restrictions};
        let path = Expr::parse_pratt(stream, Precedence::Call, Restrictions::NONE)?;
        let op = if stream.parse::<T![=]>().is_ok() {
            SetterOp::Assign
        } else {
            match stream.parse::<AssignOpKind>()? {
                AssignOpKind::AddEq => SetterOp::Increment,
                AssignOpKind::SubEq => SetterOp::Decrement,
                other => {
                    return Err(TokenError::CustomError {
                        msg: format!(
                            "UPDATE setters currently support only `=`, `+=`, and `-=`; found `{other}`"
                        ),
                        span: path.span,
                    });
                }
            }
        };
        let value = stream.parse::<Expr>()?;
        Ok(Self { path, op, value })
    }
}

impl UpdateQ {
    pub fn span(&self) -> Span {
        self.span
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokenizer::TokenKind;

    #[test]
    fn test_update_statement() {
        let input = "    
    update users@u:User {
        status: 'senior',
        discount: 10,
        score: score + 1
    }
    where u.id == 'User:3'
    return users@x[*].{
        id,
        name: concat(x.name, 4)
     };
";

        let input = "
            update users@u:User set {
                contacts[where contacts.name == 'email'].value = '4';
                contacts[5].value = 'S';
                contacts[where contacts.name == 'email'][0].name = 'S';
                contacts[where contacts.name == 'email'][4..6].meta = {
                    name: 'S'
                };
                info = {
                    name: 'Oye',
                    age: 123,
                    contact: {
                        email: 'oyelowo.oss@gmail.com',
                        phone: '08012345678'
                    },
                }
            }
            where u.age > 5
            return users@x[*].{
                id,
                name: concat(x.name, 4)
            }
        ";
    }

    #[test]
    fn test_update_simple_set() {
        use crate::Interner;
        let input = "update users@u:User set u.name = 'Jane' where u.id == 1";
        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(input, &mut interner).unwrap();
        let result = UpdateQ::parse(&mut stream);
        assert!(result.is_ok(), "Failed to parse: {:?}", result.err());
    }

    #[test]
    fn test_update_setter_accepts_increment_and_decrement_ops() {
        use crate::Interner;

        let input = "update users@u:User set { u.age += 1; u.score -= 2 } where u.id == 1";
        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(input, &mut interner).unwrap();
        let query = UpdateQ::parse(&mut stream).expect("update should parse");

        let UpdateMutation::Set(setters) = query.mutation else {
            panic!("expected set mutation");
        };

        assert_eq!(setters.len(), 2);
        assert_eq!(setters[0].op, SetterOp::Increment);
        assert_eq!(setters[1].op, SetterOp::Decrement);
    }

    #[test]
    fn test_update_content_header_does_not_eat_mutation_object() {
        use crate::Interner;

        // Regression test: with string-literal types allowed (for `Pick<T, "k">`), the
        // `{ name: 'Jane' }` mutation object must not be consumed as a structural *type*
        // during header parsing.
        let input = "update users@u:User { name: 'Jane' } where u.id == 1";

        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(input, &mut interner).unwrap();
        let q = UpdateQ::parse(&mut stream).expect("update should parse");

        assert!(q.condition.is_some(), "expected WHERE clause to parse");
        match q.mutation {
            UpdateMutation::Merge(obj) => {
                assert_eq!(obj.fields.len(), 1);
            }
            other => panic!("expected Merge(Object) mutation, got: {other:?}"),
        }
    }
}
