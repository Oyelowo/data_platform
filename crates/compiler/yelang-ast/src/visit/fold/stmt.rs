use crate::Program as ItemProgram;
use crate::{
    common::{self, *},
    expr::{self, *},
    item::{self, *},
    pattern::{self, *},
    ptr::{self, *},
    query::{self, *},
    stmt::{self, *},
    tokenizer::{self, *},
    types::{self, *},
    visit::fold::folder::Folder,
};

use crate::stmt::{LetStmt, Stmt, StmtKind};

pub fn fold_program<F: Folder + ?Sized>(f: &mut F, node: ItemProgram) -> ItemProgram {
    ItemProgram {
        items: node.items.into_iter().map(|i| f.fold_item(i)).collect(),
        span: node.span,
    }
}

pub fn fold_stmt<F: Folder + ?Sized>(f: &mut F, node: Stmt) -> Stmt {
    let kind = match node.kind {
        StmtKind::Expr(e) => StmtKind::Expr(Box::new(f.fold_expr(*e))),
        StmtKind::TermExpr(e) => StmtKind::TermExpr(Box::new(f.fold_expr(*e))),
        StmtKind::Let(l) => StmtKind::Let(Box::new(f.fold_let_stmt(*l))),
        StmtKind::Item(i) => StmtKind::Item(Box::new(f.fold_item(*i))),
        StmtKind::Empty => StmtKind::Empty,
        StmtKind::MacroInvocation(i) => StmtKind::MacroInvocation(crate::expr::MacroInvocation {
            path: f.fold_path(i.path),
            args: i.args,
            span: i.span,
        }),
    };

    Stmt {
        kind,
        span: node.span,
    }
}

pub fn fold_let_stmt<F: Folder + ?Sized>(f: &mut F, node: LetStmt) -> LetStmt {
    LetStmt {
        pattern: Box::new(f.fold_pattern(*node.pattern)),
        ty: node.ty.map(|t| Box::new(f.fold_type(*t))),
        init: node.init.map(|e| Box::new(f.fold_expr(*e))),
        span: node.span,
        attrs: node.attrs,
    }
}
