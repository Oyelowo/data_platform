/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 21/02/2025
 */

use super::Expr;
use crate::Type;

/// Runtime type assertion
// <expr> "as" <typespec> e.g. 42 as i32
#[derive(Debug, Clone, PartialEq)]
pub struct TypeCast {
    pub base: Box<Expr>,
    pub ty: Type,
}

impl TypeCast {}
