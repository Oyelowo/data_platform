/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 24/03/2025
 */

use super::Expr;
use crate::T;

/// Represents the try (`?`) operator in the AST.
///
/// This is used to *propagate* `Result`/`Option`-style control flow via the
/// language `Try` trait desugaring (see HIR lowering).
///
/// Note: this is **not** null-safe access. The language surface does not support `null`.
/// Examples: `read_file()?`, `maybe_value?`.
#[derive(Debug, Clone, PartialEq)]
pub struct TrySafeAccess {
    pub base: Box<Expr>,
    pub op: T![?],
}

impl TrySafeAccess {}
