/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 26/02/2025
 */

use super::Expr;
use crate::Pattern;
use crate::T;
use yelang_lexer::{ParseTokenStream, Span, match_map};

#[derive(Debug, Clone, PartialEq)]
pub struct AssignEqExpr {
    pub target: Box<Expr>,
    pub value: Box<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DestructureAssignExpr {
    pub pattern: Pattern,
    pub value: Box<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AssignOp {
    pub op: AssignOpKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AssignOpKind {
    /// +=
    AddEq,
    /// -=
    SubEq,
    /// *=
    MulEq,
    /// /=
    DivEq,
    /// %=
    ModEq,
    /// &=
    BitAndEq,
    /// |=
    BitOrEq,
    /// ^=
    BitXorEq,
    /// <<=
    BitShlEq,
    /// >>=
    BitShrEq,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AssignOpExpr {
    pub target: Box<Expr>,
    pub op: AssignOpKind,
    pub value: Box<Expr>,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for AssignOpKind {
    fn parse(
        stream: &mut yelang_lexer::TokenStream<crate::tokenizer::TokenKind>,
    ) -> yelang_lexer::TokenResult<Self> {
        use AssignOpKind::*;

        // IMPORTANT: Multi-character patterns MUST come before their single-character components
        // e.g., <<= before <=, >>= before >=
        let op = match_map!(
            stream,
            // Three-char operators (MUST BE FIRST)
            T![<<=] => |_| BitShlEq,
            T![>>=] => |_| BitShrEq,

            // Two-token compound assignment operators
            T![+=] => |_| AddEq,
            T![-=] => |_| SubEq,
            T![*=] => |_| MulEq,
            T![/=] => |_| DivEq,
            T![%=] => |_| ModEq,
            T![&=] => |_| BitAndEq,
            T![|=] => |_| BitOrEq,
            T![^=] => |_| BitXorEq
        );
        op
    }
}

impl std::fmt::Display for AssignOpKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AssignOpKind::AddEq => write!(f, "+="),
            AssignOpKind::SubEq => write!(f, "-="),
            AssignOpKind::MulEq => write!(f, "*="),
            AssignOpKind::DivEq => write!(f, "/="),
            AssignOpKind::ModEq => write!(f, "%="),
            AssignOpKind::BitAndEq => write!(f, "&="),
            AssignOpKind::BitOrEq => write!(f, "|="),
            AssignOpKind::BitXorEq => write!(f, "^="),
            AssignOpKind::BitShlEq => write!(f, "<<="),
            AssignOpKind::BitShrEq => write!(f, ">>="),
        }
    }
}
