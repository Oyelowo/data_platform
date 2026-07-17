//! HIR visitor trait with default `walk_*` implementations.

use crate::crate_hir::Crate;
use crate::hir::{
    Arm, Block, Expr, FieldDef, FnSig, Impl, Item, ItemKind, Stmt, StructField, Trait, Ty,
    VariantData, VariantDef,
};
use crate::hir_body::Body;
use crate::hir_pat::Pat;
use crate::ids::{BodyId, ExprId, PatId, StmtId, TyId};

/// Visitor over the HIR.
///
/// Implementations that need to traverse arena-allocated nodes should override
/// [`crate_hir`](Visitor::crate_hir) to return the `Crate` being visited. The
/// default `walk_*` helpers use this lookup to recurse through expressions,
/// patterns, statements, types, and bodies.
pub trait Visitor<'hir>: Sized {
    /// The crate being visited. Return `Some` to enable recursive traversal of
    /// arena-allocated nodes, or `None` to visit only the top-level item tree.
    fn crate_hir(&self) -> Option<&'hir Crate> {
        None
    }

    fn visit_crate(&mut self) {
        if let Some(crate_hir) = self.crate_hir() {
            walk_crate(self, crate_hir);
        }
    }

    fn visit_item(&mut self, item: &'hir Item) {
        walk_item(self, item)
    }

    fn visit_expr(&mut self, expr: &'hir Expr) {
        walk_expr(self, expr)
    }

    fn visit_stmt(&mut self, stmt: &'hir Stmt) {
        walk_stmt(self, stmt)
    }

    fn visit_ty(&mut self, ty: &'hir Ty) {
        walk_ty(self, ty)
    }

    fn visit_pat(&mut self, pat: &'hir Pat) {
        walk_pat(self, pat)
    }

    fn visit_body(&mut self, body: &'hir Body) {
        walk_body(self, body)
    }

    fn visit_block(&mut self, block: &'hir Block) {
        walk_block(self, block)
    }

    fn visit_arm(&mut self, arm: &'hir Arm) {
        walk_arm(self, arm)
    }

    fn visit_impl(&mut self, impl_: &'hir Impl) {
        walk_impl(self, impl_)
    }

    fn visit_trait(&mut self, trait_: &'hir Trait) {
        walk_trait(self, trait_)
    }

    fn visit_variant_def(&mut self, variant: &'hir VariantDef) {
        walk_variant_def(self, variant)
    }

    fn visit_field_def(&mut self, field: &'hir FieldDef) {
        walk_field_def(self, field)
    }

    fn visit_struct_field(&mut self, field: &'hir StructField) {
        walk_struct_field(self, field)
    }
}

pub fn walk_crate<'hir, V: Visitor<'hir>>(visitor: &mut V, crate_hir: &'hir Crate) {
    for item in crate_hir.items.values() {
        if let Some(item) = item {
            visitor.visit_item(item);
        }
    }
    for impl_ in &crate_hir.impls {
        visitor.visit_impl(impl_);
    }
}

pub fn walk_item<'hir, V: Visitor<'hir>>(visitor: &mut V, item: &'hir Item) {
    match &item.kind {
        ItemKind::Fn { sig, body, .. } => {
            walk_fn_sig(visitor, sig);
            visitor.visit_body_by_id(*body);
        }
        ItemKind::Struct { data, .. } | ItemKind::Union { data, .. } => {
            walk_variant_data(visitor, data);
        }
        ItemKind::Enum { def, .. } => {
            for variant in &def.variants {
                visitor.visit_variant_def(variant);
            }
        }
        ItemKind::Impl {
            items,
            ..
        } => {
            for impl_item in items {
                match &impl_item.kind {
                    crate::hir::ImplItemKind::Fn { sig, body } => {
                        walk_fn_sig(visitor, sig);
                        visitor.visit_body_by_id(*body);
                    }
                    crate::hir::ImplItemKind::Const { ty, body } => {
                        visitor.visit_ty_by_id(*ty);
                        visitor.visit_body_by_id(*body);
                    }
                    crate::hir::ImplItemKind::Type { ty } => {
                        visitor.visit_ty_by_id(*ty);
                    }
                }
            }
        }
        _ => {}
    }
}

pub fn walk_expr<'hir, V: Visitor<'hir>>(visitor: &mut V, expr: &'hir Expr) {
    match expr {
        Expr::Binary { left, right, .. } => {
            visitor.visit_expr_by_id(*left);
            visitor.visit_expr_by_id(*right);
        }
        Expr::Unary { expr: inner, .. } => {
            visitor.visit_expr_by_id(*inner);
        }
        Expr::Call { func, args } => {
            visitor.visit_expr_by_id(*func);
            for arg in args {
                visitor.visit_expr_by_id(*arg);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            visitor.visit_expr_by_id(*receiver);
            for arg in args {
                visitor.visit_expr_by_id(*arg);
            }
        }
        Expr::Field { expr: inner, .. } => {
            visitor.visit_expr_by_id(*inner);
        }
        Expr::Index { expr: inner, index } => {
            visitor.visit_expr_by_id(*inner);
            visitor.visit_expr_by_id(*index);
        }
        Expr::Assign { left, right } => {
            visitor.visit_expr_by_id(*left);
            visitor.visit_expr_by_id(*right);
        }
        Expr::Block { block } | Expr::Loop { block, .. } => {
            visitor.visit_block(block);
        }
        Expr::Break { expr, .. } => {
            if let Some(e) = expr {
                visitor.visit_expr_by_id(*e);
            }
        }
        Expr::Return { expr } => {
            if let Some(e) = expr {
                visitor.visit_expr_by_id(*e);
            }
        }
        Expr::Match { expr, arms } => {
            visitor.visit_expr_by_id(*expr);
            for arm in arms {
                visitor.visit_arm(arm);
            }
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            visitor.visit_expr_by_id(*cond);
            visitor.visit_expr_by_id(*then_branch);
            if let Some(e) = else_branch {
                visitor.visit_expr_by_id(*e);
            }
        }
        Expr::Closure { params, body, .. } => {
            for param in params {
                visitor.visit_pat_by_id(param.pat);
                visitor.visit_ty_by_id(param.ty);
            }
            visitor.visit_body_by_id(*body);
        }
        Expr::Struct { fields, rest, .. } => {
            for field in fields {
                visitor.visit_expr_by_id(field.expr);
            }
            if let Some(e) = rest {
                visitor.visit_expr_by_id(*e);
            }
        }
        Expr::Tuple { exprs } | Expr::Array { exprs } => {
            for e in exprs {
                visitor.visit_expr_by_id(*e);
            }
        }
        Expr::Cast { expr: inner, ty } => {
            visitor.visit_expr_by_id(*inner);
            visitor.visit_ty_by_id(*ty);
        }
        Expr::Let { pat, expr: inner } => {
            visitor.visit_pat_by_id(*pat);
            visitor.visit_expr_by_id(*inner);
        }
        _ => {}
    }
}

pub fn walk_stmt<'hir, V: Visitor<'hir>>(visitor: &mut V, stmt: &'hir Stmt) {
    match stmt {
        Stmt::Expr { expr } => visitor.visit_expr_by_id(*expr),
        Stmt::Let { pat, ty, init } => {
            visitor.visit_pat_by_id(*pat);
            if let Some(t) = ty {
                visitor.visit_ty_by_id(*t);
            }
            if let Some(e) = init {
                visitor.visit_expr_by_id(*e);
            }
        }
        Stmt::Item { item } => visitor.visit_item(item),
    }
}

pub fn walk_block<'hir, V: Visitor<'hir>>(visitor: &mut V, block: &'hir Block) {
    for stmt in &block.stmts {
        visitor.visit_stmt_by_id(*stmt);
    }
    if let Some(expr) = &block.expr {
        visitor.visit_expr_by_id(*expr);
    }
}

pub fn walk_arm<'hir, V: Visitor<'hir>>(visitor: &mut V, arm: &'hir Arm) {
    visitor.visit_pat_by_id(arm.pat);
    if let Some(guard) = &arm.guard {
        visitor.visit_expr_by_id(*guard);
    }
    visitor.visit_expr_by_id(arm.body);
}

pub fn walk_body<'hir, V: Visitor<'hir>>(visitor: &mut V, body: &'hir Body) {
    for param in &body.params {
        visitor.visit_pat_by_id(param.pat);
        visitor.visit_ty_by_id(param.ty);
    }
    visitor.visit_expr_by_id(body.value);
}

pub fn walk_ty<'hir, V: Visitor<'hir>>(visitor: &mut V, ty: &'hir Ty) {
    match ty {
        Ty::Tuple { tys } => {
            for t in tys {
                visitor.visit_ty_by_id(*t);
            }
        }
        Ty::Array { ty: inner, .. } | Ty::Slice { ty: inner } => {
            visitor.visit_ty_by_id(*inner);
        }
        Ty::FnPtr { sig } => {
            walk_fn_sig(visitor, sig);
        }
        Ty::AnonStruct { fields } => {
            for field in fields {
                visitor.visit_ty_by_id(field.ty);
            }
        }
        Ty::Utility { args, .. } => {
            for arg in args {
                visitor.visit_ty_by_id(*arg);
            }
        }
        Ty::Ref { ty: inner, .. } | Ty::RawPtr { ty: inner, .. } => {
            visitor.visit_ty_by_id(*inner);
        }
        Ty::ForAll { ty: inner, .. } => {
            visitor.visit_ty_by_id(*inner);
        }
        _ => {}
    }
}

pub fn walk_pat<'hir, V: Visitor<'hir>>(visitor: &mut V, pat: &'hir Pat) {
    match pat {
        Pat::Binding { subpat, .. } => {
            if let Some(p) = subpat {
                visitor.visit_pat_by_id(*p);
            }
        }
        Pat::Struct { fields, .. } => {
            for field in fields {
                visitor.visit_pat_by_id(field.pat);
            }
        }
        Pat::Tuple { pats } | Pat::TupleStruct { pats, .. } => {
            for p in pats {
                visitor.visit_pat_by_id(*p);
            }
        }
        Pat::Range { start, end, .. } => {
            if let Some(s) = start {
                visitor.visit_pat_by_id(*s);
            }
            if let Some(e) = end {
                visitor.visit_pat_by_id(*e);
            }
        }
        Pat::Or { pats } => {
            for p in pats {
                visitor.visit_pat_by_id(*p);
            }
        }
        Pat::Slice {
            prefix,
            middle,
            suffix,
        } => {
            for p in prefix {
                visitor.visit_pat_by_id(*p);
            }
            if let Some(m) = middle {
                visitor.visit_pat_by_id(*m);
            }
            for p in suffix {
                visitor.visit_pat_by_id(*p);
            }
        }
        _ => {}
    }
}

pub fn walk_fn_sig<'hir, V: Visitor<'hir>>(visitor: &mut V, sig: &'hir FnSig) {
    for ty in &sig.inputs {
        visitor.visit_ty_by_id(*ty);
    }
    visitor.visit_ty_by_id(sig.output);
}

pub fn walk_variant_data<'hir, V: Visitor<'hir>>(visitor: &mut V, data: &'hir VariantData) {
    match data {
        VariantData::Struct { fields } => {
            for field in fields {
                visitor.visit_field_def(field);
            }
        }
        VariantData::Tuple { fields } => {
            for field in fields {
                visitor.visit_struct_field(field);
            }
        }
        VariantData::Unit => {}
    }
}

pub fn walk_variant_def<'hir, V: Visitor<'hir>>(visitor: &mut V, variant: &'hir VariantDef) {
    walk_variant_data(visitor, &variant.data);
}

pub fn walk_field_def<'hir, V: Visitor<'hir>>(visitor: &mut V, field: &'hir FieldDef) {
    visitor.visit_ty_by_id(field.ty);
}

pub fn walk_struct_field<'hir, V: Visitor<'hir>>(visitor: &mut V, field: &'hir StructField) {
    visitor.visit_ty_by_id(field.ty);
}

pub fn walk_impl<'hir, V: Visitor<'hir>>(visitor: &mut V, impl_: &'hir Impl) {
    visitor.visit_ty_by_id(impl_.self_ty);
    for item in &impl_.items {
        match &item.kind {
            crate::hir::ImplItemKind::Fn { sig, body } => {
                walk_fn_sig(visitor, sig);
                visitor.visit_body_by_id(*body);
            }
            crate::hir::ImplItemKind::Const { ty, body } => {
                visitor.visit_ty_by_id(*ty);
                visitor.visit_body_by_id(*body);
            }
            crate::hir::ImplItemKind::Type { ty } => {
                visitor.visit_ty_by_id(*ty);
            }
        }
    }
}

pub fn walk_trait<'hir, V: Visitor<'hir>>(visitor: &mut V, trait_: &'hir Trait) {
    for item in &trait_.items {
        match &item.kind {
            crate::hir::TraitItemKind::Fn { sig, default } => {
                walk_fn_sig(visitor, sig);
                if let Some(body) = *default {
                    visitor.visit_body_by_id(body);
                }
            }
            crate::hir::TraitItemKind::Const { ty, body } => {
                visitor.visit_ty_by_id(*ty);
                if let Some(body) = *body {
                    visitor.visit_body_by_id(body);
                }
            }
            crate::hir::TraitItemKind::Type { bounds: _, default } => {
                // TraitBound has no nested HIR nodes to walk (only a resolved path).
                if let Some(ty) = default {
                    visitor.visit_ty_by_id(*ty);
                }
            }
        }
    }
}

// -----------------------------------------------------------------------------
// ID-based visitor helpers
// -----------------------------------------------------------------------------

/// Extension trait that adds ID-based lookup helpers to any visitor.
///
/// The default implementations look up the node in [`crate_hir`](Visitor::crate_hir)
/// and call the corresponding reference-based visitor hook. Visitors that do
/// not provide a crate simply skip the subtree.
pub trait VisitorExt<'hir>: Visitor<'hir> {
    fn visit_expr_by_id(&mut self, id: ExprId) {
        if let Some(crate_hir) = self.crate_hir() {
            if let Some(expr) = crate_hir.exprs.get(id) {
                self.visit_expr(expr);
            }
        }
    }

    fn visit_pat_by_id(&mut self, id: PatId) {
        if let Some(crate_hir) = self.crate_hir() {
            if let Some(pat) = crate_hir.pats.get(id) {
                self.visit_pat(pat);
            }
        }
    }

    fn visit_stmt_by_id(&mut self, id: StmtId) {
        if let Some(crate_hir) = self.crate_hir() {
            if let Some(stmt) = crate_hir.stmts.get(id) {
                self.visit_stmt(stmt);
            }
        }
    }

    fn visit_ty_by_id(&mut self, id: TyId) {
        if let Some(crate_hir) = self.crate_hir() {
            if let Some(ty) = crate_hir.tys.get(id) {
                self.visit_ty(ty);
            }
        }
    }

    fn visit_body_by_id(&mut self, id: BodyId) {
        if let Some(crate_hir) = self.crate_hir() {
            if let Some(body) = crate_hir.bodies.get(id) {
                self.visit_body(body);
            }
        }
    }
}

impl<'hir, T: Visitor<'hir>> VisitorExt<'hir> for T {}
