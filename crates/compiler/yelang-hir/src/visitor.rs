//! HIR visitor trait with default `walk_*` implementations.

use crate::crate_hir::Crate;
use crate::hir::{
    Arm, Block, Expr, ExprKind, FnSig, Impl, Item, ItemKind, Stmt, StmtKind,
    Trait, Ty, TyKind,
};
use crate::hir_body::Body;
use crate::hir_pat::{Pat, PatKind};
use crate::ids::BodyId;

/// Visitor over the HIR.
pub trait Visitor<'hir>: Sized {
    fn visit_crate(&mut self, crate_hir: &Crate) {
        walk_crate(self, crate_hir)
    }

    fn visit_item(&mut self, item: &Item) {
        walk_item(self, item)
    }

    fn visit_expr(&mut self, expr: &Expr) {
        walk_expr(self, expr)
    }

    fn visit_stmt(&mut self, stmt: &Stmt) {
        walk_stmt(self, stmt)
    }

    fn visit_ty(&mut self, ty: &Ty) {
        walk_ty(self, ty)
    }

    fn visit_pat(&mut self, pat: &Pat) {
        walk_pat(self, pat)
    }

    fn visit_body(&mut self, body: &Body) {
        walk_body(self, body)
    }

    fn visit_block(&mut self, block: &Block) {
        walk_block(self, block)
    }

    fn visit_arm(&mut self, arm: &Arm) {
        walk_arm(self, arm)
    }

    fn visit_impl(&mut self, impl_: &Impl) {
        walk_impl(self, impl_)
    }

    fn visit_trait(&mut self, trait_: &Trait) {
        walk_trait(self, trait_)
    }

    /// Look up a body by `BodyId`.  Default returns `None`.
    /// Concrete visitors that hold a `&Crate` can override this.
    fn visit_body_by_id(&mut self, _body_id: BodyId) -> Option<&'hir Body> {
        None
    }
}

pub fn walk_crate<'hir, V: Visitor<'hir>>(visitor: &mut V, crate_hir: &Crate) {
    for item in crate_hir.items.values() {
        if let Some(item) = item {
            visitor.visit_item(item);
        }
    }
    for impl_ in &crate_hir.impls {
        visitor.visit_impl(impl_);
    }
}

pub fn walk_item<'hir, V: Visitor<'hir>>(visitor: &mut V, item: &Item) {
    match &item.kind {
        ItemKind::Fn { sig, body, .. } => {
            walk_fn_sig(visitor, sig);
            if let Some(body) = visitor.visit_body_by_id(*body) {
                visitor.visit_body(body);
            }
        }
        ItemKind::Struct { data: _, .. } => {
            // TODO: walk fields
        }
        ItemKind::Enum { def, .. } => {
            for _variant in &def.variants {
                // TODO: walk variant data
            }
        }
        ItemKind::Impl {
            items,
            self_ty,
            of_trait,
            ..
        } => {
            visitor.visit_ty(self_ty);
            if let Some(_trait_ref) = of_trait {
                // TODO: walk trait ref
            }
            for impl_item in items {
                match &impl_item.kind {
                    crate::hir::ImplItemKind::Fn { sig, body } => {
                        walk_fn_sig(visitor, sig);
                        if let Some(body) = visitor.visit_body_by_id(*body) {
                            visitor.visit_body(body);
                        }
                    }
                    crate::hir::ImplItemKind::Const { ty, body } => {
                        visitor.visit_ty(ty);
                        if let Some(body) = visitor.visit_body_by_id(*body) {
                            visitor.visit_body(body);
                        }
                    }
                    crate::hir::ImplItemKind::Type { ty } => {
                        visitor.visit_ty(ty);
                    }
                }
            }
        }
        _ => {}
    }
}

pub fn walk_expr<'hir, V: Visitor<'hir>>(visitor: &mut V, expr: &Expr) {
    match &expr.kind {
        ExprKind::Binary { left, right, .. } => {
            visitor.visit_expr(left);
            visitor.visit_expr(right);
        }
        ExprKind::Unary { expr: inner, .. } => {
            visitor.visit_expr(inner);
        }
        ExprKind::Call { func, args } => {
            visitor.visit_expr(func);
            for arg in args {
                visitor.visit_expr(arg);
            }
        }
        ExprKind::MethodCall { receiver, args, .. } => {
            visitor.visit_expr(receiver);
            for arg in args {
                visitor.visit_expr(arg);
            }
        }
        ExprKind::Field { expr: inner, .. } => {
            visitor.visit_expr(inner);
        }
        ExprKind::Index { expr: inner, index } => {
            visitor.visit_expr(inner);
            visitor.visit_expr(index);
        }
        ExprKind::Assign { left, right } => {
            visitor.visit_expr(left);
            visitor.visit_expr(right);
        }
        ExprKind::Block { block } | ExprKind::Loop { block, .. } => {
            visitor.visit_block(block);
        }
        ExprKind::Break { expr, .. } => {
            if let Some(e) = expr {
                visitor.visit_expr(e);
            }
        }
        ExprKind::Return { expr } => {
            if let Some(e) = expr {
                visitor.visit_expr(e);
            }
        }
        ExprKind::Match { expr, arms } => {
            visitor.visit_expr(expr);
            for arm in arms {
                visitor.visit_arm(arm);
            }
        }
        ExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            visitor.visit_expr(cond);
            visitor.visit_expr(then_branch);
            if let Some(e) = else_branch {
                visitor.visit_expr(e);
            }
        }
        ExprKind::Closure { params, body, .. } => {
            for param in params {
                visitor.visit_pat(&param.pat);
                visitor.visit_ty(&param.ty);
            }
            if let Some(body) = visitor.visit_body_by_id(*body) {
                visitor.visit_body(body);
            }
        }
        ExprKind::Struct { fields, rest, .. } => {
            for field in fields {
                visitor.visit_expr(&field.expr);
            }
            if let Some(e) = rest {
                visitor.visit_expr(e);
            }
        }
        ExprKind::Tuple { exprs } | ExprKind::Array { exprs } => {
            for e in exprs {
                visitor.visit_expr(e);
            }
        }
        ExprKind::Cast { expr: inner, ty } => {
            visitor.visit_expr(inner);
            visitor.visit_ty(ty);
        }
        ExprKind::Let { pat, expr: inner } => {
            visitor.visit_pat(pat);
            visitor.visit_expr(inner);
        }
        _ => {}
    }
}

pub fn walk_stmt<'hir, V: Visitor<'hir>>(visitor: &mut V, stmt: &Stmt) {
    match &stmt.kind {
        StmtKind::Expr { expr } => visitor.visit_expr(expr),
        StmtKind::Let { pat, ty, init } => {
            visitor.visit_pat(pat);
            if let Some(t) = ty {
                visitor.visit_ty(t);
            }
            if let Some(e) = init {
                visitor.visit_expr(e);
            }
        }
        StmtKind::Item { item } => visitor.visit_item(item),
    }
}

pub fn walk_block<'hir, V: Visitor<'hir>>(visitor: &mut V, block: &Block) {
    for stmt in &block.stmts {
        visitor.visit_stmt(stmt);
    }
    if let Some(expr) = &block.expr {
        visitor.visit_expr(expr);
    }
}

pub fn walk_arm<'hir, V: Visitor<'hir>>(visitor: &mut V, arm: &Arm) {
    visitor.visit_pat(&arm.pat);
    if let Some(guard) = &arm.guard {
        visitor.visit_expr(guard);
    }
    visitor.visit_expr(&arm.body);
}

pub fn walk_body<'hir, V: Visitor<'hir>>(visitor: &mut V, body: &Body) {
    for param in &body.params {
        visitor.visit_pat(&param.pat);
        visitor.visit_ty(&param.ty);
    }
    visitor.visit_expr(&body.value);
}

pub fn walk_ty<'hir, V: Visitor<'hir>>(visitor: &mut V, ty: &Ty) {
    match &ty.kind {
        TyKind::Tuple { tys } => {
            for t in tys {
                visitor.visit_ty(t);
            }
        }
        TyKind::Array { ty: inner, .. } | TyKind::Slice { ty: inner } => {
            visitor.visit_ty(inner);
        }
        TyKind::FnPtr { sig } => {
            walk_fn_sig(visitor, sig);
        }
        TyKind::AnonStruct { fields } => {
            for field in fields {
                visitor.visit_ty(&field.ty);
            }
        }
        TyKind::Utility { args, .. } => {
            for arg in args {
                visitor.visit_ty(arg);
            }
        }
        TyKind::Ref { ty: inner, .. } | TyKind::RawPtr { ty: inner, .. } => {
            visitor.visit_ty(inner);
        }
        _ => {}
    }
}

pub fn walk_pat<'hir, V: Visitor<'hir>>(visitor: &mut V, pat: &Pat) {
    match &pat.kind {
        PatKind::Binding { subpat, .. } => {
            if let Some(p) = subpat {
                visitor.visit_pat(p);
            }
        }
        PatKind::Struct { fields, .. } => {
            for field in fields {
                visitor.visit_pat(&field.pat);
            }
        }
        PatKind::Tuple { pats } | PatKind::TupleStruct { pats, .. } => {
            for p in pats {
                visitor.visit_pat(p);
            }
        }
        PatKind::Range { start, end, .. } => {
            if let Some(s) = start {
                visitor.visit_pat(s);
            }
            if let Some(e) = end {
                visitor.visit_pat(e);
            }
        }
        PatKind::Or { pats } => {
            for p in pats {
                visitor.visit_pat(p);
            }
        }
        PatKind::Slice {
            prefix,
            middle,
            suffix,
        } => {
            for p in prefix {
                visitor.visit_pat(p);
            }
            if let Some(m) = middle {
                visitor.visit_pat(m);
            }
            for p in suffix {
                visitor.visit_pat(p);
            }
        }
        _ => {}
    }
}

pub fn walk_fn_sig<'hir, V: Visitor<'hir>>(visitor: &mut V, sig: &FnSig) {
    for ty in &sig.inputs {
        visitor.visit_ty(ty);
    }
    visitor.visit_ty(&sig.output);
}

pub fn walk_impl<'hir, V: Visitor<'hir>>(visitor: &mut V, impl_: &Impl) {
    visitor.visit_ty(&impl_.self_ty);
    for item in &impl_.items {
        match &item.kind {
            crate::hir::ImplItemKind::Fn { sig, body } => {
                walk_fn_sig(visitor, sig);
                if let Some(body) = visitor.visit_body_by_id(*body) {
                    visitor.visit_body(body);
                }
            }
            crate::hir::ImplItemKind::Const { ty, body } => {
                visitor.visit_ty(ty);
                if let Some(body) = visitor.visit_body_by_id(*body) {
                    visitor.visit_body(body);
                }
            }
            crate::hir::ImplItemKind::Type { ty } => {
                visitor.visit_ty(ty);
            }
        }
    }
}

pub fn walk_trait<'hir, V: Visitor<'hir>>(visitor: &mut V, trait_: &Trait) {
    for item in &trait_.items {
        match &item.kind {
            crate::hir::TraitItemKind::Fn { sig, default } => {
                walk_fn_sig(visitor, sig);
                if let Some(body) = default.and_then(|b| visitor.visit_body_by_id(b)) {
                    visitor.visit_body(body);
                }
            }
            crate::hir::TraitItemKind::Const { ty, body } => {
                visitor.visit_ty(ty);
                if let Some(body) = body.and_then(|b| visitor.visit_body_by_id(b)) {
                    visitor.visit_body(body);
                }
            }
            crate::hir::TraitItemKind::Type { bounds, default } => {
                for _bound in bounds {
                    // TODO: walk bound
                }
                if let Some(ty) = default {
                    visitor.visit_ty(ty);
                }
            }
        }
    }
}
