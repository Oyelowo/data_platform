/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 31/12/2024
 */

use crate::{BlockExpr, Expr, Label, Pattern, T};
use yelang_lexer::{ParseTokenStream, TokenResult, TokenStream, Verify};

/// For loop expression: `for pattern in iterator { body }`
///
/// Iterates over elements of an iterator, binding each element to a pattern.
///
/// # Example
/// ```
/// for item in items {
///     process(item);
/// }
///
/// for (key, value) in map {
///     println!("{}: {}", key, value);
/// }
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ForLoopExpr {
    pub label: Option<Label>,
    /// The pattern to bind each element to
    pub pat: Pattern,
    /// The expression that produces the iterator
    pub iter: Box<Expr>,
    /// The body of the loop containing statements and an optional final expression
    pub body: BlockExpr,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for ForLoopExpr {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let label = stream.parse::<Option<(Label, T![:])>>()?;
        stream.parse::<T![for]>()?;
        let pat = stream.parse::<Pattern>()?;
        stream.parse::<T![in]>()?;

        // Parse the iterator expression with struct literals forbidden.
        // Otherwise `for x in items { ... }` can misparse as iter=`items { ... }` (StructExpr)
        // and then fail when trying to parse the actual loop body.
        let iter = Expr::parse_pratt(
            stream,
            super::Precedence::None,
            super::Restrictions::NO_STRUCT,
        )?;

        let _ver = stream.parse::<Verify<T!['{']>>()?;
        let body = stream.parse::<BlockExpr>()?;

        Ok(ForLoopExpr {
            label: label.map(|(label, _)| label),
            pat,
            iter: Box::new(iter),
            body,
        })
    }
}

impl ForLoopExpr {}

//
// #[cfg(test)]
// mod tests {
//     use crate::lexer::TokenizeChars;
//     use super::*;
//     use crate::ast::tokenizer::TokenKind;
//
//     #[test]
//     fn test_if_stmt() {
//         let input = "
//         for x in items {
//             let y = x + 1;
//             for x in items {
//                 let y = x + 1;
//             }
//             if x > 1 {
//                 let y = x + 1;
//
//                 for x in items {
//                     let y = x + 1;
//                 }
//
//             }
//         }
//         ";
//         let mut stream = TokenKind::tokenize(input).unwrap();
//         let stmt = stream.parse::<ForLoopExpr>().unwrap();
//         // panic!("{:#?}", stmt);
//
//         // assert_eq!(stmt.if_block.exprs.len(), 0);
//         // assert_eq!(stmt.else_if_branches.len(), 3);
//         // assert_eq!(stmt.else_block.unwrap().exprs.len(), 0);
//     }
// }
