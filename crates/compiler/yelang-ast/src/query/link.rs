/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 31/12/2024
 */

use super::EdgeDirection;
use super::select::Node;
use crate::{Expr, Ident, Object, T, TokenKind, Type};
use yelang_lexer::{ParseTokenStream, SeparatedList, Span, TokenError, TokenResult, TokenStream};

// consider update link statement or just make link upsert so u dnt need a separate update statement
#[derive(Debug, Clone, PartialEq)]
pub struct LinkQ {
    // Allow multiple separate paths: LINK (a)->(b), (c)->(d)
    pub paths: Vec<CreatePath>,
    pub return_: Option<Expr>,
    pub span: Span,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for LinkQ {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let checkpoint = stream.checkpoint();
        stream.parse::<T![link]>()?;

        let is_block = matches!(stream.peek().map(|t| t.kind()), Some(TokenKind::OpenBrace));

        let (paths, return_expr) = if is_block {
            stream.parse::<T!['{']>()?;
            let paths = stream
                .parse::<SeparatedList<CreatePath, T![,], true>>()?
                .value_owned();

            let tail = stream
                .parse::<Option<(T![;], Expr)>>()?
                .map(|(_, expr)| expr);

            stream.parse::<T!['}']>()?;

            (paths, tail)
        } else {
            let paths = stream
                .parse::<SeparatedList<CreatePath, T![,], true>>()?
                .value_owned();
            (paths, None)
        };

        Ok(Self {
            paths,
            return_: return_expr,
            span: stream.span_since(checkpoint),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CreatePathSegment {
    Node(Node),
    Edge(CreateEdge),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CreatePath {
    pub segments: Vec<CreatePathSegment>,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for CreatePath {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let create_path_checkpoint = stream.checkpoint();
        let mut segments = Vec::new();

        let start = stream.parse::<Node>()?;
        segments.push(CreatePathSegment::Node(start));

        // Parse one-or-more: `DIR [edge] DIR (node)`.
        // NOTE: This is written in a more declarative style so it remains compatible
        // with future error-recovery work.
        // let rest =
        //     stream.parse::<RepeatMin<1, (EdgeDirection, CreateEdge, EdgeDirection, Node)>>()?;

        // for (direction_left, mut edge, direction_right, target) in rest.value_owned() { }

        // We require the direction token on both sides of the edge to match.
        let mut parsed_any = false;
        loop {
            let checkpoint = stream.checkpoint();

            let Ok(direction_left) = stream.parse::<EdgeDirection>() else {
                stream.restore(checkpoint);
                break;
            };

            let mut edge = stream.parse::<CreateEdge>()?;
            let direction_right = stream.parse::<EdgeDirection>()?;

            if direction_left != direction_right {
                return Err(TokenError::CustomError {
                    msg: format!(
                        "LINK path direction tokens must match: expected `{0}` after edge, found `{1}`",
                        direction_left, direction_right
                    ),
                    span: stream.span_since(checkpoint),
                });
            }

            let target = stream.parse::<Node>()?;
            edge.direction = direction_left;
            segments.push(CreatePathSegment::Edge(edge));
            segments.push(CreatePathSegment::Node(target));
            parsed_any = true;
        }

        if !parsed_any {
            return Err(TokenError::CustomError {
                msg: "LINK path must contain at least one edge segment".to_string(),
                span: stream.span_since(create_path_checkpoint),
            });
        }

        Ok(Self { segments })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CreateEdge {
    pub var: Ident,
    pub binding: Ident,
    pub table: Type,  // Table is REQUIRED for creation
    pub data: Object, // Data payload object; braces required (use `{}` when empty)
    pub direction: EdgeDirection,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for CreateEdge {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        // Parse: [Table] or [var: Table] followed by REQUIRED Data payload object
        // (use `{}` when you have no edge properties).
        type EdgeBody = (Ident, T![@], Ident, T![:], Type); // [var@v: Table]

        let (_, (var, _at, binding, _col, table), data, _) =
            stream.parse::<(T!['['], EdgeBody, Object, T![']'])>()?;

        Ok(Self {
            var,
            binding,
            table,
            data,
            direction: EdgeDirection::Forward, // Set later
        })
    }
}

impl LinkQ {
    pub fn span(&self) -> Span {
        self.span
    }
}

// enum LinkSegmentConnectionCreation {
//     Node(NodeDef),
//     Edge(EdgeDefWithData),
// }

impl LinkQ {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Interner;
    use crate::tokenizer::TokenKind;
    use yelang_lexer::ParseTokenStream;

    #[test]
    fn test_link_statement() {
        let input = "
            link {
                (user:User) -> [follows:UserFollowsUser {
                    since: now(),
                    mutual: false
                }] -> (target:User),

                (user) -> [follows:UserFollowsUser {
                    since: now(),
                    mutual: false
                }] -> (target:User)

                user[WHERE user.age > 20].{
                    id,
                    name: concat(user.name, 4)
                }
            }
        ";

        // let mut stream = TokenKind::tokenize(input).unwrap();
        // let stmt = stream.parse::<All<LinkStatement>>().unwrap();
        // panic!("{:#?}", stmt);
        // assert_eq!(stmt.label, Some(Ident::new_unchecked("user")));
    }

    #[test]
    fn link_allows_multiple_paths_in_statement_form() {
        let input = r#"
            link (users@u where u.id == 1) -> [follows@f:UserFollowsUser {}] -> (users@v where v.id == 2),
                 (users@u where u.id == 1) -> [writes@w:UserWritesBook {}] -> (books@b where b.id == 10);
        "#;

        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(input, &mut interner).unwrap();

        let q = LinkQ::parse(&mut stream).expect("link should parse");
        assert_eq!(q.paths.len(), 2);

        // In statement form, the trailing semicolon is not consumed by `LinkQ` itself.
        stream
            .parse::<T![;]>()
            .expect("expected trailing semicolon");
    }
}

//  link (user:User:123) -> [follows:UserFollowsUser {
//   since: time::now(),
//   mutual: false
// }] -> (target:User:456);

//  link (user:User:123) -> [studies_at:UserStudiesAtUniversity] -> (university:University:456);
//
//  link (user:User[where user.age > 18]) -> [enrolled:UserEnrolledInCourse] -> (course:Course);
//
//  link (user:User:123) -> [follows:UserFollowsUser {
//   since: time::now(),
//   mutual: false
// }] -> (target:User:456);
//
//  link (user:User:123) <-> [friends_with:UserFriendsWithUser] <-> (friend:User:456);
// link (user:User where user.id == "u123")
//   to (book:Book where book.id == "b456")
//   value UserWritesBook {
//     published_date: dt'2020-01-01',
//     review_score: 4.5
//   }
//   return {
//     user_id: user.id,
//     book_id: book.id,
//     published_date,
//     review_score
//   };
//
//
// ---
//
//
