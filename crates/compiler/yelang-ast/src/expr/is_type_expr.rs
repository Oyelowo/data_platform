/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 09/11/2025
 */

use crate::{Expr, Type};

#[derive(Debug, Clone, PartialEq)]
pub struct IsTypeExpr {
    pub expr: Box<Expr>,
    pub ty: Type,
}

impl IsTypeExpr {}
