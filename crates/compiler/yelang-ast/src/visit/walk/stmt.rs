use crate::stmt::{LetStmt, Stmt, StmtKind};
use crate::{Program, visit::walk::visitor::Visitor};
use std::ops::ControlFlow;

pub fn walk_program<V: Visitor>(v: &mut V, program: &Program) -> ControlFlow<()> {
    for item in &program.items {
        v.visit_item(item)?;
    }
    ControlFlow::Continue(())
}

pub fn walk_stmt<V: Visitor>(v: &mut V, stmt: &Stmt) -> ControlFlow<()> {
    match &stmt.kind {
        StmtKind::Expr(e) | StmtKind::TermExpr(e) => v.visit_expr(&*e),
        StmtKind::Let(l) => v.visit_let_stmt(l),
        StmtKind::Item(i) => v.visit_item(i),
        StmtKind::Empty => ControlFlow::Continue(()),
    }
}

pub fn walk_let_stmt<V: Visitor>(v: &mut V, let_stmt: &LetStmt) -> ControlFlow<()> {
    v.visit_pattern(&let_stmt.pattern)?;
    if let Some(ty) = &let_stmt.ty {
        v.visit_type(ty)?;
    }
    if let Some(init) = &let_stmt.init {
        v.visit_expr(&*init)?;
    }
    ControlFlow::Continue(())
}
