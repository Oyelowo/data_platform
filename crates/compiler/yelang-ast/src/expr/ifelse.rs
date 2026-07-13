/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 31/12/2024
 */

use crate::{BlockExpr, Expr, ExprKind, T};
use yelang_lexer::{Either, ParseTokenStream, TokenResult, TokenStream};

#[derive(Debug, Clone, PartialEq)]
pub struct IfExpr {
    pub condition: Box<Expr>,
    pub then_block: BlockExpr,
    pub else_expr: Option<Box<Expr>>,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for IfExpr {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let checkpoint = stream.checkpoint();

        stream.parse::<T![if]>()?;

        // Parse condition with struct literals forbidden to prevent ambiguity:
        // `if opt { ... }` should NOT parse `opt { ... }` as a struct literal
        let condition = Expr::parse_pratt(
            stream,
            super::Precedence::None,
            super::Restrictions::NO_STRUCT,
        )?;

        let then_block = stream.parse::<BlockExpr>()?;
        let (else_expr, else_span) =
            stream.parse_with_span::<Option<(T![else], Either<Self, BlockExpr>)>>()?;

        let else_expr = else_expr
            .map(|(_, expr)| match expr {
                Either::Left(if_expr) => Expr {
                    kind: ExprKind::If(if_expr),
                    span: else_span,
                },
                Either::Right(block_expr) => Expr {
                    kind: ExprKind::Block(block_expr),
                    span: else_span,
                },
            })
            .map(Box::new);

        let _span = stream.span_since(checkpoint);

        Ok(IfExpr {
            condition: Box::new(condition),
            then_block,
            else_expr,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::Interner;

    use super::*;
    use crate::tokenizer::TokenKind;

    #[test]
    fn test_if_stmt() {
        let input = "
        // let x = 0u32;
        // let a = 4;
        let m = ri#`lo. {dt'2023-05-25T00:00:00Z'} {{{du'5w1d55m' + { hali + 1 } }}} wo{a + 1 + du'5w1d55m' -  i'ggg {q}' } dayo...`#;

        // let normal_string = ri##'423e4567-e89b-12d3-a456-426614174000'##;
        // let uuid = uuid'423e4567-e89b-12d3-a456-426614174000';
        // let uuid = uuid-r##'423e4567-e89b-12d3-a456-426614174000'##;
        // let duration = du'5w1d55m';
        // let duration = du-r'5w1d55m';
        // let date = dt'2023-05-25T00:00:00Z';
        // let date = dt'2023-05-25T00:00:00Z';
        // let date = geo'POINT(30 10)';
        // if get_it(45, second) {
        //     let x = 1;
        //     let m = x + 1;
        //     56u8
        // }
        // else if false {
        //     let y = 2;
        // }
        // else if second {
        //     let y = 2;
        // }
        // else {
        //     let z = 3u8;
        // }
";

        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(input, &mut interner).unwrap();
        // panic!("{:#?}.........{:#?}", interner, stream);
        // let stmt = stream.parse::<IfExpr>().unwrap();
        // panic!("{:#?}", stmt);

        // assert_eq!(stmt.if_block.exprs.len(), 0);
        // assert_eq!(stmt.else_if_branches.len(), 3);
        // assert_eq!(stmt.else_block.unwrap().exprs.len(), 0);
    }

    #[test]
    fn test_simple_if_with_let() {
        let input = "if let Some(x) = opt { x }";

        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(input, &mut interner).unwrap();
        let if_expr = stream.parse::<IfExpr>().unwrap();

        // Condition should be a Let expression
        assert!(matches!(if_expr.condition.kind, ExprKind::Let(_)));
    }

    #[test]
    fn test_if_with_regular_condition() {
        let input = "if x > 5 { do_something() }";

        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(input, &mut interner).unwrap();
        let if_expr = stream.parse::<IfExpr>().unwrap();

        // Condition should be a binary expression
        assert!(matches!(if_expr.condition.kind, ExprKind::Binary(_)));
    }

    #[test]
    fn test_if_let_with_qualified_tuple_struct_pattern_and_closure_call_body() {
        let input = r#"
            if let Option::Some(limit) = opt {
                xs.probe_any(|x| x > limit)
            } else {
                false
            }
        "#;

        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(input, &mut interner).unwrap();
        let if_expr = stream.parse::<IfExpr>().unwrap();

        assert!(matches!(if_expr.condition.kind, ExprKind::Let(_)));
    }
}
