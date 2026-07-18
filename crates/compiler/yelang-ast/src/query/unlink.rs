/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 31/12/2024
 */

use super::{LinkPath, parse_query_tail};
use crate::{Expr, T, TokenKind};
use yelang_lexer::{ParseTokenStream, SeparatedList, TokenResult, TokenStream};

#[derive(Debug, Clone, PartialEq)]
pub struct UnlinkQ {
    pub paths: Vec<LinkPath>,
    pub return_: Option<Expr>,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for UnlinkQ {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let checkpoint = stream.checkpoint();
        stream.parse::<T![unlink]>()?;

        let is_block = matches!(stream.peek().map(|t| t.kind()), Some(TokenKind::OpenBrace));

        if is_block {
            stream.parse::<T!['{']>()?;
            type MultiPaths = SeparatedList<LinkPath, T![,], true>;
            let (links, _) = stream.parse::<(MultiPaths, Option<T![,]>)>()?;
            let tail = parse_query_tail(stream)?;
            stream.parse::<T!['}']>()?;

            Ok(UnlinkQ {
                paths: links.value_owned(),
                return_: tail,
            })
        } else {
            type MultiPaths = SeparatedList<LinkPath, T![,], true>;
            let (links, _) = stream.parse::<(MultiPaths, Option<T![,]>)>()?;
            Ok(UnlinkQ {
                paths: links.value_owned(),
                return_: None,
            })
        }
    }
}

impl UnlinkQ {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Stmt, tokenizer::TokenKind};
    use yelang_lexer::{All, TokenizeChars};

    #[test]
    fn test_create_statement() {
        let input = "
    UNLINK  
     (user:User) -> [follows:UserFollowsUser WHERE follows.since < now() - 1] -> (target:User) -> [eats:UserEatsFood WHERE eats.since < now() - 1] -> (food:Food),
     (user:User) -> [writes:WritesBlog WHERE writes.published_date > dt'2020-01-01'] -> (blog:Blog WHERE blog.views > 10000)
    return user[where user.age > 20].{
        id,
        name: concat(user.name, 4)
    };";

        // let mut stream = TokenKind::tokenize(input).unwrap();
        // let stmt = stream.parse::<All<Stmt>>().unwrap();
        // stream.reset_dangerous();
        // let stmt = stream.parse::<Unlink>().is_ok();
        // stream.reset_dangerous();
        // let stmt = stream.parse::<All<Unlink>>().is_err();
        // panic!("{:#?}", stmt);
        // assert_eq!(stmt.label, Some(Ident::new_unchecked("user")));
    }
}
// UNLINK  (user:User:123) -> [follows:UserFollowsUser WHERE follows.since < time::now() - 1y] -> (target:User:456);
//
// UNLINK
// (user:User:123) -> [follows:UserFollowsUser WHERE follows.since < time::now() - 1y] -> (target:User:456),
// (user) -> [eats:UserEatsFood WHERE eats.since < time::now() - 1y] -> (food:Food:456),

// LINK (user:User:123) -> [follows:UserFollowsUser {
//   since: time::now(),
//   mutual: false
// }] -> (target:User:456);

// LINK (user:User:123) -> [studies_at:UserStudiesAtUniversity] -> (university:University:456);
//
// LINK (user:User[WHERE user.age > 18]) -> [enrolled:UserEnrolledInCourse] -> (course:Course);
//
// LINK (user:User:123) -> [follows:UserFollowsUser {
//   since: time::now(),
//   mutual: false
// }] -> (target:User:456);
//
// LINK (user:User:123) <-> [friends_with:UserFriendsWithUser] <-> (friend:User:456);
// link (user:User WHERE user.id == "u123")
//   to (book:Book WHERE book.id == "b456")
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
