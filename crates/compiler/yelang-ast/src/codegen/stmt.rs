use crate::{Codegen, Interner};
use crate::{LetStmt, Stmt, StmtKind};
use std::fmt::{self, Write};

// --- Statements ---

impl Codegen for Stmt {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        match &self.kind {
            StmtKind::Expr(expr) => expr.codegen(f, interner),
            StmtKind::TermExpr(expr) => {
                expr.codegen(f, interner)?;
                write!(f, ";")
            }
            StmtKind::Let(let_stmt) => let_stmt.codegen(f, interner),
            StmtKind::Item(item) => item.codegen(f, interner),
            StmtKind::Empty => Ok(()),
        }
    }
}

// --- Let Statements ---

impl Codegen for LetStmt {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        write!(f, "let ")?;
        self.pattern.codegen(f, interner)?;
        if let Some(init) = &self.init {
            write!(f, " = ")?;
            init.codegen(f, interner)?;
        }
        write!(f, ";")
    }
}
