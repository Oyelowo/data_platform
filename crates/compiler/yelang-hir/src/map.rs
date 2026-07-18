//! HIR node lookup map (like rustc's `hir::map`).

use crate::crate_data::Crate;
use crate::hir::core::{Expr, Item, Stmt};
use crate::hir::body::Body;
use crate::hir::pat::Pat;
use crate::hir::ty::Ty;
use crate::ids::{BodyId, DefId, ExprId, PatId, StmtId, HirTyId};

/// Provides O(1) lookup from HIR ids to HIR nodes.
pub struct Map<'hir> {
    pub crate_hir: &'hir Crate,
}

impl<'hir> Map<'hir> {
    pub fn new(crate_hir: &'hir Crate) -> Self {
        Self { crate_hir }
    }

    /// Lookup an item by `DefId`.
    pub fn item(&self, def_id: DefId) -> Option<&Item> {
        self.crate_hir.items.get(def_id).and_then(|opt| opt.as_ref())
    }

    /// Lookup a body by `BodyId`.
    pub fn body(&self, body_id: BodyId) -> Option<&Body> {
        self.crate_hir.body(body_id)
    }

    /// Lookup an expression by `ExprId`.
    pub fn expr(&self, expr_id: ExprId) -> Option<&Expr> {
        self.crate_hir.expr(expr_id)
    }

    /// Lookup a HIR type by `HirTyId`.
    pub fn ty(&self, ty_id: HirTyId) -> Option<&Ty> {
        self.crate_hir.ty(ty_id)
    }

    /// Lookup a pattern by `PatId`.
    pub fn pat(&self, pat_id: PatId) -> Option<&Pat> {
        self.crate_hir.pat(pat_id)
    }

    /// Lookup a statement by `StmtId`.
    pub fn stmt(&self, stmt_id: StmtId) -> Option<&Stmt> {
        self.crate_hir.stmt(stmt_id)
    }
}
