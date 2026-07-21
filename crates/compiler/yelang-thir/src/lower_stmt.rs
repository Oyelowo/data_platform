//! Statement lowering: HIR `Stmt` → THIR `ThirStmt`.

use yelang_hir::ids::StmtId;

use crate::context::LoweringContext;
use crate::errors::LoweringError;
use crate::ids::ThirStmtId;
use crate::stmt::ThirStmt;

impl<'a> LoweringContext<'a> {
    /// Lower a HIR statement to a THIR statement.
    pub fn lower_stmt(&mut self, stmt_id: StmtId) -> Result<ThirStmtId, LoweringError> {
        let Some(stmt) = self.hir.stmt(stmt_id) else {
            let err = self.alloc_expr(crate::expr::ThirExpr::Err);
            return Ok(self.alloc_stmt(ThirStmt::Expr { expr: err }));
        };

        let thir_stmt = match stmt {
            yelang_hir::hir::core::Stmt::Expr { expr } => ThirStmt::Expr {
                expr: self.lower_expr(*expr)?,
            },
            yelang_hir::hir::core::Stmt::Let { pat, ty: _, init } => ThirStmt::Let {
                pat: self.lower_pat(*pat),
                ty: self.pat_ty(*pat),
                init: self.lower_opt_expr(*init)?,
            },
            yelang_hir::hir::core::Stmt::Item { item } => ThirStmt::Item {
                def_id: item.def_id,
            },
        };

        Ok(self.alloc_stmt(thir_stmt))
    }
}
