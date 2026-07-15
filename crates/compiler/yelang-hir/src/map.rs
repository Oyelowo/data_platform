//! HIR node lookup map (like rustc's `hir::map`).

use crate::crate_hir::Crate;
use crate::hir::{Expr, Item, Pat, Stmt, Ty};
use crate::hir_body::Body;
use crate::ids::{BodyId, DefId, HirId};

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
        self.crate_hir.items.get(&def_id)
    }

    /// Lookup a body by `BodyId`.
    pub fn body(&self, body_id: BodyId) -> Option<&Body> {
        self.crate_hir.bodies.get(&body_id)
    }

    /// Lookup an expression by `HirId`.
    ///
    /// This walks all bodies because expressions are not stored in a top-level map.
    pub fn expr(&self, hir_id: HirId) -> Option<&Expr> {
        for (_id, body) in self.crate_hir.bodies.iter() {
            if let Some(expr) = find_expr_in_expr(&body.value, hir_id) {
                return Some(expr);
            }
        }
        None
    }

    /// Lookup a type by `HirId`.
    pub fn ty(&self, _hir_id: HirId) -> Option<&Ty> {
        // Types are embedded inside expressions/items; a full implementation
        // would build an index.  For now, return `None`.
        None
    }

    /// Lookup a pattern by `HirId`.
    pub fn pat(&self, _hir_id: HirId) -> Option<&Pat> {
        // Patterns are embedded inside expressions/items; a full implementation
        // would build an index.  For now, return `None`.
        None
    }
}

fn find_expr_in_expr<'a>(expr: &'a Expr, target: HirId) -> Option<&'a Expr> {
    if expr.hir_id == target {
        return Some(expr);
    }
    match &expr.kind {
        crate::hir::ExprKind::Binary { left, right, .. } => {
            find_expr_in_expr(left, target).or_else(|| find_expr_in_expr(right, target))
        }
        crate::hir::ExprKind::Unary { expr: inner, .. } => find_expr_in_expr(inner, target),
        crate::hir::ExprKind::Call { func, args } => find_expr_in_expr(func, target)
            .or_else(|| args.iter().find_map(|arg| find_expr_in_expr(arg, target))),
        crate::hir::ExprKind::MethodCall { receiver, args, .. } => {
            find_expr_in_expr(receiver, target)
                .or_else(|| args.iter().find_map(|arg| find_expr_in_expr(arg, target)))
        }
        crate::hir::ExprKind::Field { expr: inner, .. } => find_expr_in_expr(inner, target),
        crate::hir::ExprKind::Index { expr: inner, index } => {
            find_expr_in_expr(inner, target).or_else(|| find_expr_in_expr(index, target))
        }
        crate::hir::ExprKind::Assign { left, right } => {
            find_expr_in_expr(left, target).or_else(|| find_expr_in_expr(right, target))
        }
        crate::hir::ExprKind::Block { block } => block
            .stmts
            .iter()
            .find_map(|stmt| find_expr_in_stmt(stmt, target))
            .or_else(|| {
                block
                    .expr
                    .as_ref()
                    .and_then(|e| find_expr_in_expr(e, target))
            }),
        crate::hir::ExprKind::Loop { block, .. } => block
            .stmts
            .iter()
            .find_map(|stmt| find_expr_in_stmt(stmt, target))
            .or_else(|| {
                block
                    .expr
                    .as_ref()
                    .and_then(|e| find_expr_in_expr(e, target))
            }),
        crate::hir::ExprKind::Break { expr, .. } => {
            expr.as_ref().and_then(|e| find_expr_in_expr(e, target))
        }
        crate::hir::ExprKind::Return { expr } => {
            expr.as_ref().and_then(|e| find_expr_in_expr(e, target))
        }
        crate::hir::ExprKind::Match { expr, arms } => {
            find_expr_in_expr(expr, target).or_else(|| {
                arms.iter().find_map(|arm| {
                    arm.guard
                        .as_ref()
                        .and_then(|g| find_expr_in_expr(g, target))
                        .or_else(|| find_expr_in_expr(&arm.body, target))
                })
            })
        }
        crate::hir::ExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => find_expr_in_expr(cond, target)
            .or_else(|| find_expr_in_expr(then_branch, target))
            .or_else(|| {
                else_branch
                    .as_ref()
                    .and_then(|e| find_expr_in_expr(e, target))
            }),
        crate::hir::ExprKind::Closure { .. } => None, // body is in a separate Body
        crate::hir::ExprKind::Struct { fields, rest, .. } => fields
            .iter()
            .find_map(|f| find_expr_in_expr(&f.expr, target))
            .or_else(|| rest.as_ref().and_then(|e| find_expr_in_expr(e, target))),
        crate::hir::ExprKind::Tuple { exprs } => {
            exprs.iter().find_map(|e| find_expr_in_expr(e, target))
        }
        crate::hir::ExprKind::Array { exprs } => {
            exprs.iter().find_map(|e| find_expr_in_expr(e, target))
        }
        crate::hir::ExprKind::Cast { expr: inner, .. } => find_expr_in_expr(inner, target),
        crate::hir::ExprKind::Let { expr: inner, .. } => find_expr_in_expr(inner, target),
        _ => None,
    }
}

fn find_expr_in_stmt<'a>(stmt: &'a Stmt, target: HirId) -> Option<&'a Expr> {
    match &stmt.kind {
        crate::hir::StmtKind::Expr { expr } => find_expr_in_expr(expr, target),
        crate::hir::StmtKind::Let { init, .. } => {
            init.as_ref().and_then(|e| find_expr_in_expr(e, target))
        }
        crate::hir::StmtKind::Item { .. } => None,
    }
}
