/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 21/02/2025
 */

use crate::{ExprKind, T, TokenKind};
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use yelang_lexer::{ParseTokenStream, Span, TokenResult, TokenStream, match_map};

use super::{Associativity, Expr, Precedence, PrecedenceExt};
use crate::Codegen;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BinaryOp {
    // Arithmetic operators
    /// +
    Add,
    /// -
    Subtract,
    /// *
    Multiply,
    /// /
    Divide,
    /// %
    Modulo,
    /// **
    Power,

    // Comparison operators
    Eq,    // =
    Ne,    // !=
    Gt,    // >
    Lt,    // <
    Gte,   // >=
    Lte,   // <=
    Like,  // ~~
    ILike, // ~~*
    Regex, // =~

    In,
    NotIn,

    // Logical operators
    // Boolean logic with short-circuit awareness
    And,
    Or,
    // Xor,

    // Bitwise operators
    /// &
    BitAnd,
    /// |
    BitOr,
    /// ^
    BitXor,
    /// <<
    Shl,
    /// >>
    Shr,
}

impl BinaryOp {
    // pub fn negate(&self) -> Option<Self> {
    //     use BinaryOp::*;
    //     match self {
    //         Add => Some(Subtract),
    //         Subtract => Some(Add),
    //         Multiply => Some(Divide),
    //         Divide => Some(Multiply),
    //         Modulo => Some(Modulo),
    //         Power => None,
    //         Eq => Some(Ne),
    //         Ne => Some(Eq),
    //         Gt => Some(Lte),
    //         Lt => Some(Gte),
    //         Gte => Some(Lt),
    //         Lte => Some(Gt),
    //         Like => None,
    //         ILike => None,
    //         Regex => None,
    //         In => None,
    //         NotIn => None,
    //         And => Some(Or),
    //         Or => Some(And),
    //     }
    // }

    // Helper to negate comparison operators for ALL -> AntiJoin conversion
    pub fn negate(&self) -> Option<BinaryOp> {
        match self {
            BinaryOp::Eq => Some(BinaryOp::Ne),
            BinaryOp::Ne => Some(BinaryOp::Eq),
            BinaryOp::Gt => Some(BinaryOp::Lte),
            BinaryOp::Gte => Some(BinaryOp::Lt),
            BinaryOp::Lt => Some(BinaryOp::Gte),
            BinaryOp::Lte => Some(BinaryOp::Gt),
            // Cannot simply negate logical or arithmetic operators in this context
            _ => None,
        }
    }

    pub fn is_binary(&self) -> bool {
        matches!(
            self,
            BinaryOp::Add
                | BinaryOp::Subtract
                | BinaryOp::Multiply
                | BinaryOp::Divide
                | BinaryOp::Eq
                | BinaryOp::Ne
                | BinaryOp::Gt
                | BinaryOp::Lt
                | BinaryOp::Gte
                | BinaryOp::Lte
                | BinaryOp::Like
                | BinaryOp::ILike
                | BinaryOp::Regex
                | BinaryOp::In
                | BinaryOp::NotIn
                | BinaryOp::And
                | BinaryOp::Or
        )
    }

    pub fn is_assign_eq(&self) -> bool {
        false
    }

    pub fn is_assign_op(&self) -> bool {
        false
    }
}

impl PrecedenceExt for BinaryOp {
    fn precedence(&self) -> Precedence {
        use BinaryOp::*;
        match self {
            Add | Subtract => Precedence::Term,
            Multiply | Divide | Modulo => Precedence::Factor,
            Eq | Ne => Precedence::Equality,
            In | NotIn => Precedence::Membership,
            Gt | Lt | Gte | Lte | Like | ILike | Regex => Precedence::Comparison,
            And => Precedence::LogicalAnd,
            Or => Precedence::LogicalOr,
            Power => Precedence::Exponent,
            BitAnd => Precedence::BitwiseAnd,
            BitOr => Precedence::BitwiseOr,
            BitXor => Precedence::BitwiseXor,
            Shl | Shr => Precedence::BitShift,
        }
    }

    fn associativity(&self) -> Associativity {
        use BinaryOp::*;

        match self {
            And | Or | Add | Subtract | Multiply | Divide | Modulo | Eq | Ne | Gt | Lt | Gte
            | Lte | Like | ILike | Regex | BitAnd | BitOr | BitXor | Shl | Shr => {
                Associativity::Left
            }
            Power => Associativity::Right,
            In | NotIn => Associativity::NonAssociative,
        }
    }
}

impl Display for BinaryOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use BinaryOp::*;
        match self {
            // Arithmetic operators
            Add => write!(f, "+"),
            Subtract => write!(f, "-"),
            Power => write!(f, "**"),
            Multiply => write!(f, "*"),
            Divide => write!(f, "/"),
            Modulo => write!(f, "%"),

            // Comparison operators
            Eq => write!(f, "=="),
            Ne => write!(f, "!="),
            Gt => write!(f, ">"),
            Lt => write!(f, "<"),
            Gte => write!(f, ">="),
            Lte => write!(f, "<="),
            Like => write!(f, "~~"),
            ILike => write!(f, "~~*"),
            Regex => write!(f, "=~"),

            In => write!(f, "in"),
            NotIn => write!(f, "not in"),

            // Logical operators
            And => write!(f, "&&"),
            Or => write!(f, "||"),

            // Bitwise operators
            BitAnd => write!(f, "&"),
            BitOr => write!(f, "|"),
            BitXor => write!(f, "^"),
            Shl => write!(f, "<<"),
            Shr => write!(f, ">>"),
        }
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for BinaryOp {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        use BinaryOp::*;

        // IMPORTANT: Multi-character patterns MUST come before their single-character components
        // e.g., || before |, << before <, >> before >, ** before *, not in before in
        let op = match_map!(
            stream,
            // Multi-character operators (MUST BE FIRST)
            (T![not], T![in]) => |_| NotIn,
            (T![|], T![|]) => |_| Or,
            (T![*], T![*]) => |_| Power,
            (T![<], T![<]) => |_| Shl,
            (T![>], T![>]) => |_| Shr,

            // Comparison operators (two-char tokens)
            T![==] => |_| Eq,
            T![!=] => |_| Ne,
            T![>=] => |_| Gte,
            T![<=] => |_| Lte,

            // Arithmetic operators
            T![+] => |_| Add,
            T![-] => |_| Subtract,
            T![*] => |_| Multiply,
            T![/] => |_| Divide,
            T![%] => |_| Modulo,

            // Comparison operators (single-char)
            T![>] => |_| Gt,
            T![<] => |_| Lt,

            // Logical operators
            T![in] => |_| In,
            T![and] => |_| And,

            // Bitwise operators
            T![&] => |_| BitAnd,
            T![|] => |_| BitOr,
            T![^] => |_| BitXor
            // TODO: improve/customize error message
        );
        op
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BinaryExpr {
    // e.g. "==", ">", "+", "-"
    pub op: BinaryOp,
    pub left: Box<Expr>,
    pub right: Box<Expr>,
}

// impl Display for BinaryExpr<'_> {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         write!(f, "{} {} {}", self.lhs, self.op, self.rhs)
//     }
// }

impl ParseTokenStream<crate::tokenizer::TokenKind> for BinaryExpr {
    fn parse(
        stream: &mut yelang_lexer::TokenStream<crate::tokenizer::TokenKind>,
    ) -> yelang_lexer::TokenResult<Self> {
        let (left, op, right) = stream.parse::<(Expr, BinaryOp, Expr)>()?;

        Ok(BinaryExpr {
            op,
            left: Box::new(left),
            right: Box::new(right),
        })
    }
}

impl BinaryExpr {
    pub fn left(&self) -> &Expr {
        &self.left
    }

    pub fn right(&self) -> &Expr {
        &self.right
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yelang_lexer::{TokenStream, TokenizeChars};

    // #[test]
    // fn test_parse_comparison_expr() {
    //     let mut x = 5;
    //     let y = x = 6;
    //     let mut input = Token::tokenize("1 * 2")
    //         // .inspect(|x| println!("1xxx{}", x))
    //         // .inspect_err(|x| println!("2yyy{}", x))
    //         .unwrap();
    //     let result = input
    //         .parse::<Expr>()
    //         // .inspect(|x| println!("xxx{:?}", x))
    //         // .inspect_err(|x| println!("yyy{}", x))
    //         .unwrap();
    //
    //     // panic!("result = {:#?}", result);
    //
    //     // assert_eq!(
    //     //     result,
    //     //     BinaryExpr {
    //     //         op: BinaryOp::Plus,
    //     //         lhs: Box::new(Expr::Literal(1)),
    //     //         rhs: Box::new(Expr::Literal(2)),
    //     //     }
    //     // );
    // }
}
