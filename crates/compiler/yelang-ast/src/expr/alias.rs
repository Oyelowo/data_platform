/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 24/03/2025
 */

use super::Expr;
use crate::Ident;

// Alias binds type/table to variable,
// Bindat binds another variable/expr to another variable
// typically useful within a complex statement substructure where you want to
// share data within the tree or cross-substructure e.g within projection
// or from grouping alias to projection
// so alias is an alias to type/table, bindat is an alias to another variable or maybe expr(TBD)
#[derive(Debug, Clone, PartialEq)]
pub struct BindAtExpr {
    pub base: Box<Expr>,
    pub at: Ident,
}
