/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 21/02/2025
 */
use crate::{Expr, Ident};

#[derive(Debug, Clone, PartialEq)]
pub struct MemberAccess {
    pub base: Box<Expr>,
    pub member: Ident,
}

impl MemberAccess {
    pub fn base(&self) -> &Expr {
        &self.base
    }

    pub fn member(&self) -> &Ident {
        &self.member
    }
}
