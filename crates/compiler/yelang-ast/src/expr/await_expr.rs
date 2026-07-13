/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 09/11/2025
 */

use crate::expr::Expr;
use yelang_lexer::Span;

#[derive(Debug, Clone)]
pub struct AwaitExpr {
    pub expr: Box<Expr>,
}
