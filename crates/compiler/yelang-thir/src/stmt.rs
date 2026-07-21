//! THIR statements.

use crate::ids::{ThirExprId, ThirPatId};
use crate::ty::ThirTyId;
use yelang_arena::DefId;

/// Kinds of THIR statements.
#[derive(Debug, Clone)]
pub enum ThirStmt {
    /// Expression statement.
    Expr { expr: ThirExprId },
    /// `let` binding.
    Let {
        pat: ThirPatId,
        ty: Option<ThirTyId>,
        init: Option<ThirExprId>,
    },
    /// Nested item declaration.
    Item { def_id: DefId },
}
