//! In-place mutating HIR visitor.
//!
//! A [`MutVisitor`] receives mutable references to HIR nodes and can mutate
//! them in place.  Arena-allocated nodes are reached via the `visit_*_id_mut`
//! dispatch functions, which look up the node by ID and then recursively walk
//! its children top-down.

use crate::crate_data::Crate;
use crate::hir::body::Body;
use crate::hir::core::{
    Arm, BinderParam, Block, Expr, FieldDef, FnSig, GenericParam, Generics, Impl, ImplItem, Item,
    ItemKind, Stmt, StructField, Trait, TraitBound, TraitItem, TraitRef, Ty, UsePath, VariantData,
    VariantDef, WhereClause, WherePredicate,
};
use crate::hir::pat::Pat;
use crate::hir::ty::{Const, ConstKind, GenericArg};
use crate::ids::{BodyId, DefId, ExprId, HirTyId, PatId, StmtId};
use crate::res::Res;

/// In-place mutating visitor over the HIR.
///
/// Trait methods receive mutable references to nodes and default to doing
/// nothing.  The `visit_*_id_mut` dispatch functions and `walk_*_mut` helpers
/// recursively traverse arena-allocated children after calling the trait
/// method on the parent.
pub trait MutVisitor: Sized {
    fn visit_crate(&mut self, _crate_hir: &mut Crate) {}

    fn visit_item(&mut self, _item: &mut Item) {}

    fn visit_expr(&mut self, _expr: &mut Expr) {}

    fn visit_stmt(&mut self, _stmt: &mut Stmt) {}

    fn visit_ty(&mut self, _ty: &mut Ty) {}

    fn visit_pat(&mut self, _pat: &mut Pat) {}

    fn visit_body(&mut self, _body: &mut Body) {}

    fn visit_block(&mut self, _block: &mut Block) {}

    fn visit_arm(&mut self, _arm: &mut Arm) {}

    fn visit_impl(&mut self, _impl_: &mut Impl) {}

    fn visit_impl_item(&mut self, _item: &mut ImplItem) {}

    fn visit_trait(&mut self, _trait_: &mut Trait) {}

    fn visit_trait_item(&mut self, _item: &mut TraitItem) {}

    fn visit_variant_def(&mut self, _variant: &mut VariantDef) {}

    fn visit_field_def(&mut self, _field: &mut FieldDef) {}

    fn visit_struct_field(&mut self, _field: &mut StructField) {}

    fn visit_generics(&mut self, _generics: &mut Generics) {}

    fn visit_generic_param(&mut self, _param: &mut GenericParam) {}

    fn visit_binder_param(&mut self, _param: &mut BinderParam) {}

    fn visit_where_clause(&mut self, _clause: &mut WhereClause) {}

    fn visit_where_predicate(&mut self, _predicate: &mut WherePredicate) {}

    fn visit_trait_bound(&mut self, _bound: &mut TraitBound) {}

    fn visit_trait_ref(&mut self, _trait_ref: &mut TraitRef) {}

    fn visit_use_path(&mut self, _path: &mut UsePath) {}
}

/// Walk the whole crate, mutating nodes in place.
pub fn walk_crate_mut(v: &mut impl MutVisitor, crate_hir: &mut Crate) {
    let item_keys: Vec<_> = crate_hir.items.keys().collect();
    for def_id in item_keys {
        visit_item_id_mut(v, crate_hir, def_id);
    }
    let trait_keys: Vec<_> = crate_hir.traits.keys().collect();
    for def_id in trait_keys {
        visit_trait_id_mut(v, crate_hir, def_id);
    }
    let impls = std::mem::take(&mut crate_hir.impls);
    crate_hir.impls = impls
        .into_iter()
        .map(|mut impl_| {
            v.visit_impl(&mut impl_);
            walk_impl_mut(v, crate_hir, &mut impl_);
            impl_
        })
        .collect();
}

/// Visit the item stored at `def_id` in place.
pub fn visit_item_id_mut(v: &mut impl MutVisitor, crate_hir: &mut Crate, def_id: DefId) {
    if let Some(mut item) = crate_hir
        .items
        .get_mut(def_id)
        .and_then(|o| std::mem::take(o))
    {
        v.visit_item(&mut item);
        walk_item_mut(v, crate_hir, &mut item);
        crate_hir.items[def_id] = Some(item);
    }
}

/// Visit the trait definition stored at `def_id` in place.
pub fn visit_trait_id_mut(v: &mut impl MutVisitor, crate_hir: &mut Crate, def_id: DefId) {
    if let Some(mut trait_) = crate_hir
        .traits
        .get_mut(def_id)
        .and_then(|o| std::mem::take(o))
    {
        v.visit_trait(&mut trait_);
        walk_trait_mut(v, crate_hir, &mut trait_);
        crate_hir.traits[def_id] = Some(trait_);
    }
}

/// Visit the expression at `expr_id` in place.
pub fn visit_expr_id_mut(v: &mut impl MutVisitor, crate_hir: &mut Crate, expr_id: ExprId) {
    let Some(mut expr) = crate_hir
        .exprs
        .get_mut(expr_id)
        .and_then(|slot| std::mem::take(slot))
    else {
        return;
    };
    v.visit_expr(&mut expr);
    walk_expr_mut(v, crate_hir, &mut expr);
    if let Some(slot) = crate_hir.exprs.get_mut(expr_id) {
        *slot = Some(expr);
    }
}

/// Visit the statement at `stmt_id` in place.
pub fn visit_stmt_id_mut(v: &mut impl MutVisitor, crate_hir: &mut Crate, stmt_id: StmtId) {
    let Some(mut stmt) = crate_hir
        .stmts
        .get_mut(stmt_id)
        .and_then(|slot| std::mem::take(slot))
    else {
        return;
    };
    v.visit_stmt(&mut stmt);
    walk_stmt_mut(v, crate_hir, &mut stmt);
    if let Some(slot) = crate_hir.stmts.get_mut(stmt_id) {
        *slot = Some(stmt);
    }
}

/// Visit the type at `ty_id` in place.
pub fn visit_ty_id_mut(v: &mut impl MutVisitor, crate_hir: &mut Crate, ty_id: HirTyId) {
    let Some(mut ty) = crate_hir
        .tys
        .get_mut(ty_id)
        .and_then(|slot| std::mem::take(slot))
    else {
        return;
    };
    v.visit_ty(&mut ty);
    walk_ty_mut(v, crate_hir, &mut ty);
    if let Some(slot) = crate_hir.tys.get_mut(ty_id) {
        *slot = Some(ty);
    }
}

/// Visit the pattern at `pat_id` in place.
pub fn visit_pat_id_mut(v: &mut impl MutVisitor, crate_hir: &mut Crate, pat_id: PatId) {
    let Some(mut pat) = crate_hir
        .pats
        .get_mut(pat_id)
        .and_then(|slot| std::mem::take(slot))
    else {
        return;
    };
    v.visit_pat(&mut pat);
    walk_pat_mut(v, crate_hir, &mut pat);
    if let Some(slot) = crate_hir.pats.get_mut(pat_id) {
        *slot = Some(pat);
    }
}

/// Visit the body at `body_id` in place.
pub fn visit_body_id_mut(v: &mut impl MutVisitor, crate_hir: &mut Crate, body_id: BodyId) {
    let Some(mut body) = crate_hir
        .bodies
        .get_mut(body_id)
        .and_then(|slot| std::mem::take(slot))
    else {
        return;
    };
    v.visit_body(&mut body);
    walk_body_mut(v, crate_hir, &mut body);
    if let Some(slot) = crate_hir.bodies.get_mut(body_id) {
        *slot = Some(body);
    }
}

pub fn walk_item_mut(v: &mut impl MutVisitor, crate_hir: &mut Crate, item: &mut Item) {
    match &mut item.kind {
        ItemKind::Fn {
            sig,
            body,
            generics,
        } => {
            walk_generics_mut(v, crate_hir, generics);
            walk_fn_sig_mut(v, crate_hir, sig);
            visit_body_id_mut(v, crate_hir, *body);
        }
        ItemKind::Struct { data, generics } => {
            walk_generics_mut(v, crate_hir, generics);
            walk_variant_data_mut(v, crate_hir, data);
        }
        ItemKind::Enum { def, generics } => {
            walk_generics_mut(v, crate_hir, generics);
            for variant in &mut def.variants {
                walk_variant_def_mut(v, crate_hir, variant);
            }
        }
        ItemKind::Trait {
            items,
            generics,
            super_traits,
        } => {
            walk_generics_mut(v, crate_hir, generics);
            for super_trait in super_traits {
                walk_trait_ref_mut(v, crate_hir, super_trait);
            }
            for trait_item in items {
                walk_trait_item_mut(v, crate_hir, trait_item);
            }
        }
        ItemKind::Impl {
            items,
            generics,
            self_ty,
            of_trait,
            polarity: _,
        } => {
            walk_generics_mut(v, crate_hir, generics);
            visit_ty_id_mut(v, crate_hir, *self_ty);
            if let Some(trait_ref) = of_trait {
                walk_trait_ref_mut(v, crate_hir, trait_ref);
            }
            for impl_item in items {
                walk_impl_item_mut(v, crate_hir, impl_item);
            }
        }
        ItemKind::TyAlias { ty, generics } => {
            walk_generics_mut(v, crate_hir, generics);
            visit_ty_id_mut(v, crate_hir, *ty);
        }
        ItemKind::Const { ty, body } => {
            visit_ty_id_mut(v, crate_hir, *ty);
            visit_body_id_mut(v, crate_hir, *body);
        }
        ItemKind::Static { ty, body, .. } => {
            visit_ty_id_mut(v, crate_hir, *ty);
            visit_body_id_mut(v, crate_hir, *body);
        }
        ItemKind::Mod { items } => {
            for def_id in items {
                visit_item_id_mut(v, crate_hir, *def_id);
            }
        }
        ItemKind::Use { path, .. } => {
            walk_use_path_mut(v, crate_hir, path);
        }
    }
}

pub fn walk_expr_mut(v: &mut impl MutVisitor, crate_hir: &mut Crate, expr: &mut Expr) {
    match expr {
        Expr::Binary { left, right, .. } => {
            visit_expr_id_mut(v, crate_hir, *left);
            visit_expr_id_mut(v, crate_hir, *right);
        }
        Expr::Unary { expr: inner, .. } => {
            visit_expr_id_mut(v, crate_hir, *inner);
        }
        Expr::Call { func, args } => {
            visit_expr_id_mut(v, crate_hir, *func);
            for arg in args {
                visit_expr_id_mut(v, crate_hir, *arg);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            visit_expr_id_mut(v, crate_hir, *receiver);
            for arg in args {
                visit_expr_id_mut(v, crate_hir, *arg);
            }
        }
        Expr::Field { expr: inner, .. } => {
            visit_expr_id_mut(v, crate_hir, *inner);
        }
        Expr::Index { expr: inner, index } => {
            visit_expr_id_mut(v, crate_hir, *inner);
            visit_expr_id_mut(v, crate_hir, *index);
        }
        Expr::Assign { left, right } => {
            visit_expr_id_mut(v, crate_hir, *left);
            visit_expr_id_mut(v, crate_hir, *right);
        }
        Expr::Block { block } | Expr::Loop { block, .. } => {
            walk_block_mut(v, crate_hir, block);
        }
        Expr::Break { expr, .. } => {
            if let Some(e) = expr {
                visit_expr_id_mut(v, crate_hir, *e);
            }
        }
        Expr::Return { expr } => {
            if let Some(e) = expr {
                visit_expr_id_mut(v, crate_hir, *e);
            }
        }
        Expr::Match { expr, arms } => {
            visit_expr_id_mut(v, crate_hir, *expr);
            for arm in arms {
                walk_arm_mut(v, crate_hir, arm);
            }
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            visit_expr_id_mut(v, crate_hir, *cond);
            visit_expr_id_mut(v, crate_hir, *then_branch);
            if let Some(e) = else_branch {
                visit_expr_id_mut(v, crate_hir, *e);
            }
        }
        Expr::Closure { params, body, .. } => {
            for param in params {
                visit_pat_id_mut(v, crate_hir, param.pat);
                visit_ty_id_mut(v, crate_hir, param.ty);
            }
            visit_body_id_mut(v, crate_hir, *body);
        }
        Expr::Struct { fields, rest, .. } => {
            for field in fields {
                visit_expr_id_mut(v, crate_hir, field.expr);
            }
            if let Some(e) = rest {
                visit_expr_id_mut(v, crate_hir, *e);
            }
        }
        Expr::Tuple { exprs } | Expr::Array { exprs } => {
            for e in exprs {
                visit_expr_id_mut(v, crate_hir, *e);
            }
        }
        Expr::Cast { expr: inner, ty } => {
            visit_expr_id_mut(v, crate_hir, *inner);
            visit_ty_id_mut(v, crate_hir, *ty);
        }
        Expr::Let { pat, expr: inner } => {
            visit_pat_id_mut(v, crate_hir, *pat);
            visit_expr_id_mut(v, crate_hir, *inner);
        }
        Expr::AssignOp { left, right, .. } => {
            visit_expr_id_mut(v, crate_hir, *left);
            visit_expr_id_mut(v, crate_hir, *right);
        }
        Expr::DestructureAssign { pat, value } => {
            visit_pat_id_mut(v, crate_hir, *pat);
            visit_expr_id_mut(v, crate_hir, *value);
        }
        Expr::Range { start, end, .. } => {
            if let Some(e) = start {
                visit_expr_id_mut(v, crate_hir, *e);
            }
            if let Some(e) = end {
                visit_expr_id_mut(v, crate_hir, *e);
            }
        }
        Expr::Object { fields } => {
            for field in fields {
                visit_expr_id_mut(v, crate_hir, field.expr);
            }
        }
        Expr::IsType { expr: inner, ty } => {
            visit_expr_id_mut(v, crate_hir, *inner);
            visit_ty_id_mut(v, crate_hir, *ty);
        }
        Expr::TypeAscription { expr: inner, ty } => {
            visit_expr_id_mut(v, crate_hir, *inner);
            visit_ty_id_mut(v, crate_hir, *ty);
        }
        Expr::Try { expr: inner } | Expr::Await { expr: inner } => {
            visit_expr_id_mut(v, crate_hir, *inner);
        }
        Expr::Async { body } | Expr::Gen { body, .. } => {
            visit_body_id_mut(v, crate_hir, *body);
        }
        Expr::DocumentAccess { base, projection } => {
            visit_expr_id_mut(v, crate_hir, *base);
            for proj in projection {
                match proj {
                    crate::hir::expr::DocumentProjection::Field { value, .. } => {
                        if let Some(e) = value {
                            visit_expr_id_mut(v, crate_hir, *e);
                        }
                    }
                    crate::hir::expr::DocumentProjection::Spread(e) => {
                        visit_expr_id_mut(v, crate_hir, *e);
                    }
                }
            }
        }
        Expr::Comprehension {
            element,
            variables,
            condition,
            ..
        } => {
            visit_expr_id_mut(v, crate_hir, *element);
            for var in variables {
                visit_pat_id_mut(v, crate_hir, var.pat);
                visit_expr_id_mut(v, crate_hir, var.source);
            }
            if let Some(cond) = condition {
                visit_expr_id_mut(v, crate_hir, *cond);
            }
        }
        Expr::Query(query) => match &mut query.kind {
            crate::hir::query::QueryKind::Select(select) => {
                visit_expr_id_mut(v, crate_hir, select.projection);
                for from in &mut select.from {
                    visit_expr_id_mut(v, crate_hir, from.source);
                    visit_pat_id_mut(v, crate_hir, from.binder);
                    if let Some(ty) = from.elem_ty {
                        visit_ty_id_mut(v, crate_hir, ty);
                    }
                    if let Some(filter) = from.filter {
                        visit_expr_id_mut(v, crate_hir, filter);
                    }
                    for part in &mut from.order_by {
                        visit_expr_id_mut(v, crate_hir, part.expr);
                    }
                    if let Some(range) = &mut from.range {
                        if let Some(start) = range.start {
                            visit_expr_id_mut(v, crate_hir, start);
                        }
                        if let Some(end) = range.end {
                            visit_expr_id_mut(v, crate_hir, end);
                        }
                    }
                }
                if let Some(where_clause) = select.where_clause {
                    visit_expr_id_mut(v, crate_hir, where_clause);
                }
                for part in &mut select.order_by {
                    visit_expr_id_mut(v, crate_hir, part.expr);
                }
                if let Some(range) = &mut select.range {
                    if let Some(start) = range.start {
                        visit_expr_id_mut(v, crate_hir, start);
                    }
                    if let Some(end) = range.end {
                        visit_expr_id_mut(v, crate_hir, end);
                    }
                }
            }
        },
        Expr::Lit { .. } | Expr::Path { .. } | Expr::Continue { .. } | Expr::Err => {}
    }
}

pub fn walk_stmt_mut(v: &mut impl MutVisitor, crate_hir: &mut Crate, stmt: &mut Stmt) {
    match stmt {
        Stmt::Expr { expr } => visit_expr_id_mut(v, crate_hir, *expr),
        Stmt::Let { pat, ty, init } => {
            visit_pat_id_mut(v, crate_hir, *pat);
            if let Some(t) = ty {
                visit_ty_id_mut(v, crate_hir, *t);
            }
            if let Some(e) = init {
                visit_expr_id_mut(v, crate_hir, *e);
            }
        }
        Stmt::Item { item } => {
            v.visit_item(item);
            walk_item_mut(v, crate_hir, item);
        }
    }
}

pub fn walk_block_mut(v: &mut impl MutVisitor, crate_hir: &mut Crate, block: &mut Block) {
    for stmt in &mut block.stmts {
        visit_stmt_id_mut(v, crate_hir, *stmt);
    }
    if let Some(expr) = &mut block.expr {
        visit_expr_id_mut(v, crate_hir, *expr);
    }
}

pub fn walk_arm_mut(v: &mut impl MutVisitor, crate_hir: &mut Crate, arm: &mut Arm) {
    visit_pat_id_mut(v, crate_hir, arm.pat);
    if let Some(guard) = &mut arm.guard {
        visit_expr_id_mut(v, crate_hir, *guard);
    }
    visit_expr_id_mut(v, crate_hir, arm.body);
}

pub fn walk_body_mut(v: &mut impl MutVisitor, crate_hir: &mut Crate, body: &mut Body) {
    for param in &mut body.params {
        visit_pat_id_mut(v, crate_hir, param.pat);
        visit_ty_id_mut(v, crate_hir, param.ty);
    }
    visit_expr_id_mut(v, crate_hir, body.value);
}

pub fn walk_ty_mut(v: &mut impl MutVisitor, crate_hir: &mut Crate, ty: &mut Ty) {
    match ty {
        Ty::Path { args, .. } => {
            for arg in args {
                walk_generic_arg_mut(v, crate_hir, arg);
            }
        }
        Ty::Tuple { tys } => {
            for t in tys {
                visit_ty_id_mut(v, crate_hir, *t);
            }
        }
        Ty::Array { ty: inner, len } => {
            visit_ty_id_mut(v, crate_hir, *inner);
            walk_const_mut(v, crate_hir, len);
        }
        Ty::Slice { ty: inner } => {
            visit_ty_id_mut(v, crate_hir, *inner);
        }
        Ty::FnPtr { sig } => {
            walk_fn_sig_mut(v, crate_hir, sig);
        }
        Ty::AnonStruct { fields } => {
            for field in fields {
                visit_ty_id_mut(v, crate_hir, field.ty);
            }
        }
        Ty::TypeLit { .. } => {}
        Ty::Utility { args, .. } => {
            for arg in args {
                visit_ty_id_mut(v, crate_hir, *arg);
            }
        }
        Ty::Ref { ty: inner, .. } | Ty::RawPtr { ty: inner, .. } => {
            visit_ty_id_mut(v, crate_hir, *inner);
        }
        Ty::ForAll { params, ty: inner } => {
            for param in params {
                walk_binder_param_mut(v, crate_hir, param);
            }
            visit_ty_id_mut(v, crate_hir, *inner);
        }
        Ty::Union { tys } => {
            for t in tys {
                visit_ty_id_mut(v, crate_hir, *t);
            }
        }
        Ty::TypeOf { expr } => {
            visit_expr_id_mut(v, crate_hir, *expr);
        }
        Ty::ImplTrait { .. } | Ty::DynTrait { .. } => {}
        Ty::Never | Ty::Infer | Ty::Missing | Ty::Err => {}
    }
}

pub fn walk_generic_arg_mut(v: &mut impl MutVisitor, crate_hir: &mut Crate, arg: &mut GenericArg) {
    match arg {
        GenericArg::Type(ty) => visit_ty_id_mut(v, crate_hir, *ty),
        GenericArg::Const(c) => walk_const_mut(v, crate_hir, c),
        GenericArg::AssocBinding { ty, .. } => visit_ty_id_mut(v, crate_hir, *ty),
    }
}

pub fn walk_const_mut(v: &mut impl MutVisitor, crate_hir: &mut Crate, constant: &mut Const) {
    match &mut constant.kind {
        ConstKind::Lit { .. } | ConstKind::Err => {}
        ConstKind::Expr { body } => visit_body_id_mut(v, crate_hir, *body),
    }
}

pub fn walk_pat_mut(v: &mut impl MutVisitor, crate_hir: &mut Crate, pat: &mut Pat) {
    match pat {
        Pat::Binding { subpat, .. } => {
            if let Some(p) = subpat {
                visit_pat_id_mut(v, crate_hir, *p);
            }
        }
        Pat::Struct { fields, .. } => {
            for field in fields {
                visit_pat_id_mut(v, crate_hir, field.pat);
            }
        }
        Pat::Tuple { pats } | Pat::TupleStruct { pats, .. } => {
            for p in pats {
                visit_pat_id_mut(v, crate_hir, *p);
            }
        }
        Pat::Range { start, end, .. } => {
            if let Some(s) = start {
                visit_pat_id_mut(v, crate_hir, *s);
            }
            if let Some(e) = end {
                visit_pat_id_mut(v, crate_hir, *e);
            }
        }
        Pat::Or { pats } => {
            for p in pats {
                visit_pat_id_mut(v, crate_hir, *p);
            }
        }
        Pat::Slice {
            prefix,
            middle,
            suffix,
        } => {
            for p in prefix {
                visit_pat_id_mut(v, crate_hir, *p);
            }
            if let Some(m) = middle {
                visit_pat_id_mut(v, crate_hir, *m);
            }
            for p in suffix {
                visit_pat_id_mut(v, crate_hir, *p);
            }
        }
        Pat::Ref { pat, .. } => {
            visit_pat_id_mut(v, crate_hir, *pat);
        }
        Pat::Rest { .. } | Pat::Wild | Pat::Path { .. } | Pat::Lit { .. } | Pat::Err => {}
    }
}

pub fn walk_fn_sig_mut(v: &mut impl MutVisitor, crate_hir: &mut Crate, sig: &mut FnSig) {
    for ty in &mut sig.inputs {
        visit_ty_id_mut(v, crate_hir, *ty);
    }
    visit_ty_id_mut(v, crate_hir, sig.output);
}

pub fn walk_generics_mut(v: &mut impl MutVisitor, crate_hir: &mut Crate, generics: &mut Generics) {
    for param in &mut generics.params {
        walk_generic_param_mut(v, crate_hir, param);
    }
    if let Some(where_clause) = &mut generics.where_clause {
        walk_where_clause_mut(v, crate_hir, where_clause);
    }
}

pub fn walk_generic_param_mut(
    v: &mut impl MutVisitor,
    crate_hir: &mut Crate,
    param: &mut GenericParam,
) {
    match param {
        GenericParam::Type {
            bounds, default, ..
        } => {
            for bound in bounds {
                v.visit_trait_bound(bound);
            }
            if let Some(ty) = default {
                visit_ty_id_mut(v, crate_hir, *ty);
            }
        }
        GenericParam::Const { ty, default, .. } => {
            visit_ty_id_mut(v, crate_hir, *ty);
            if let Some(expr) = default {
                visit_expr_id_mut(v, crate_hir, *expr);
            }
        }
    }
}

pub fn walk_binder_param_mut(
    v: &mut impl MutVisitor,
    crate_hir: &mut Crate,
    param: &mut BinderParam,
) {
    match param {
        BinderParam::Type { bounds, .. } => {
            for bound in bounds {
                v.visit_trait_bound(bound);
            }
        }
        BinderParam::Const { ty, .. } => {
            visit_ty_id_mut(v, crate_hir, *ty);
        }
    }
}

pub fn walk_where_clause_mut(
    v: &mut impl MutVisitor,
    crate_hir: &mut Crate,
    clause: &mut WhereClause,
) {
    for predicate in &mut clause.predicates {
        walk_where_predicate_mut(v, crate_hir, predicate);
    }
}

pub fn walk_where_predicate_mut(
    v: &mut impl MutVisitor,
    crate_hir: &mut Crate,
    predicate: &mut WherePredicate,
) {
    match predicate {
        WherePredicate::TraitBound { ty, bounds } => {
            visit_ty_id_mut(v, crate_hir, *ty);
            for bound in bounds {
                v.visit_trait_bound(bound);
            }
        }
        WherePredicate::TypeEq { lhs, rhs } => {
            visit_ty_id_mut(v, crate_hir, *lhs);
            visit_ty_id_mut(v, crate_hir, *rhs);
        }
    }
}

pub fn walk_trait_bound_mut(
    v: &mut impl MutVisitor,
    crate_hir: &mut Crate,
    bound: &mut TraitBound,
) {
    for arg in &mut bound.args {
        walk_generic_arg_mut(v, crate_hir, arg);
    }
}

pub fn walk_trait_ref_mut(
    _v: &mut impl MutVisitor,
    _crate_hir: &mut Crate,
    _trait_ref: &mut TraitRef,
) {
    // Trait references contain only a resolved path; no nested HIR nodes to walk.
}

pub fn walk_use_path_mut(v: &mut impl MutVisitor, crate_hir: &mut Crate, path: &mut UsePath) {
    v.visit_use_path(path);
    let def_id = match path.res {
        Res::Def { def_id } => Some(def_id),
        Res::SelfTy { def_id } | Res::SelfVal { def_id } => Some(def_id),
        Res::Local { .. } | Res::PrimTy { .. } | Res::Err => None,
    };
    if let Some(def_id) = def_id {
        visit_item_id_mut(v, crate_hir, def_id);
        visit_trait_id_mut(v, crate_hir, def_id);
    }
}

pub fn walk_variant_data_mut(
    v: &mut impl MutVisitor,
    crate_hir: &mut Crate,
    data: &mut VariantData,
) {
    match data {
        VariantData::Struct { fields } => {
            for field in fields {
                walk_field_def_mut(v, crate_hir, field);
            }
        }
        VariantData::Tuple { fields } => {
            for field in fields {
                walk_struct_field_mut(v, crate_hir, field);
            }
        }
        VariantData::Unit => {}
    }
}

pub fn walk_variant_def_mut(
    v: &mut impl MutVisitor,
    crate_hir: &mut Crate,
    variant: &mut VariantDef,
) {
    walk_variant_data_mut(v, crate_hir, &mut variant.data);
    if let Some(discriminant) = &mut variant.discriminant {
        walk_const_mut(v, crate_hir, discriminant);
    }
}

pub fn walk_field_def_mut(v: &mut impl MutVisitor, crate_hir: &mut Crate, field: &mut FieldDef) {
    visit_ty_id_mut(v, crate_hir, field.ty);
}

pub fn walk_struct_field_mut(
    v: &mut impl MutVisitor,
    crate_hir: &mut Crate,
    field: &mut StructField,
) {
    visit_ty_id_mut(v, crate_hir, field.ty);
}

pub fn walk_impl_mut(v: &mut impl MutVisitor, crate_hir: &mut Crate, impl_: &mut Impl) {
    walk_generics_mut(v, crate_hir, &mut impl_.generics);
    visit_ty_id_mut(v, crate_hir, impl_.self_ty);
    if let Some(trait_ref) = &mut impl_.of_trait {
        walk_trait_ref_mut(v, crate_hir, trait_ref);
    }
    for item in &mut impl_.items {
        walk_impl_item_mut(v, crate_hir, item);
    }
}

pub fn walk_impl_item_mut(v: &mut impl MutVisitor, crate_hir: &mut Crate, item: &mut ImplItem) {
    v.visit_impl_item(item);
    match &mut item.kind {
        crate::hir::core::ImplItemKind::Fn { sig, body } => {
            walk_fn_sig_mut(v, crate_hir, sig);
            visit_body_id_mut(v, crate_hir, *body);
        }
        crate::hir::core::ImplItemKind::Const { ty, body } => {
            visit_ty_id_mut(v, crate_hir, *ty);
            visit_body_id_mut(v, crate_hir, *body);
        }
        crate::hir::core::ImplItemKind::Type { ty } => {
            visit_ty_id_mut(v, crate_hir, *ty);
        }
    }
}

pub fn walk_trait_mut(v: &mut impl MutVisitor, crate_hir: &mut Crate, trait_: &mut Trait) {
    walk_generics_mut(v, crate_hir, &mut trait_.generics);
    for super_trait in &mut trait_.super_traits {
        walk_trait_ref_mut(v, crate_hir, super_trait);
    }
    for item in &mut trait_.items {
        walk_trait_item_mut(v, crate_hir, item);
    }
}

pub fn walk_trait_item_mut(v: &mut impl MutVisitor, crate_hir: &mut Crate, item: &mut TraitItem) {
    v.visit_trait_item(item);
    match &mut item.kind {
        crate::hir::core::TraitItemKind::Fn { sig, default } => {
            walk_fn_sig_mut(v, crate_hir, sig);
            if let Some(body) = default {
                visit_body_id_mut(v, crate_hir, *body);
            }
        }
        crate::hir::core::TraitItemKind::Const { ty, body } => {
            visit_ty_id_mut(v, crate_hir, *ty);
            if let Some(body) = body {
                visit_body_id_mut(v, crate_hir, *body);
            }
        }
        crate::hir::core::TraitItemKind::Type { bounds, default } => {
            for bound in bounds {
                v.visit_trait_bound(bound);
            }
            if let Some(ty) = default {
                visit_ty_id_mut(v, crate_hir, *ty);
            }
        }
    }
}
