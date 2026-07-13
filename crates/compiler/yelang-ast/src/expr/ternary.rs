/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 26/02/2025
 */

use crate::Expr;

/// A conditional (ternary) expression: condition ? if_true : if_false.
#[derive(Debug, Clone, PartialEq)]
pub struct TernaryExpr {
    pub condition: Box<Expr>,
    pub if_true: Box<Expr>,
    pub if_false: Box<Expr>,
}

// // Parsed in expr
// impl ParseTokenStream for TernaryExpr {
//     fn parse(stream: &mut TokenStream) -> TokenResult<Self> {
//         let (condition, _, if_true, _, if_false) =
//             stream.parse::<(Expr, T![?], Expr, T![:], Expr)>()?;
//
//         Ok(Self {
//             condition: Box::new(condition),
//             if_true: Box::new(if_true),
//             if_false: Box::new(if_false),
//         })
//     }
// }

impl TernaryExpr {
    /// Get the condition expression.
    pub fn condition(&self) -> &Expr {
        &self.condition
    }

    /// Get the true expression.
    pub fn if_true(&self) -> &Expr {
        &self.if_true
    }

    /// Get the false expression.
    pub fn if_false(&self) -> &Expr {
        &self.if_false
    }
}
