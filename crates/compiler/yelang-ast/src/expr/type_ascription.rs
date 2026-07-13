/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 20/03/2026
 */

use crate::{Expr, Type};

#[derive(Debug, Clone, PartialEq)]
pub struct TypeAscription {
    pub expr: Box<Expr>,
    pub ty: Type,
}

impl TypeAscription {}
