/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 09/11/2025
 */

use crate::expr::Expr;

#[derive(Debug, Clone, PartialEq)]
pub struct SpreadExpr {
    pub expr: Box<Expr>,
}
