//! HIR visitor trait with default `walk_*` implementations.

use crate::crate_data::Crate;
use crate::hir::core::{
    Arm, BinderParam, Block, Expr, FieldDef, FnSig, GenericParam, Generics, Impl, ImplItem, Item,
    ItemKind, Stmt, StructField, Trait, TraitBound, TraitItem, TraitRef, HirTy, UsePath, VariantData,
    VariantDef, WhereClause, WherePredicate,
};
use crate::hir::body::Body;
use crate::hir::pat::Pat;
use crate::hir::ty::{Const, ConstKind, GenericArg};
use crate::ids::{BodyId, ExprId, PatId, StmtId, HirTyId};
use crate::res::Res;

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

    fn visit_ty(&mut self, ty: &'hir HirTy) {
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

    fn visit_impl_item(&mut self, item: &'hir ImplItem) {
        walk_impl_item(self, item)
    }

    fn visit_trait(&mut self, trait_: &'hir Trait) {
        walk_trait(self, trait_)
    }

    fn visit_trait_item(&mut self, item: &'hir TraitItem) {
        walk_trait_item(self, item)
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

    fn visit_generics(&mut self, generics: &'hir Generics) {
        walk_generics(self, generics)
    }

    fn visit_generic_param(&mut self, param: &'hir GenericParam) {
        walk_generic_param(self, param)
    }

    fn visit_binder_param(&mut self, param: &'hir BinderParam) {
        walk_binder_param(self, param)
    }

    fn visit_where_clause(&mut self, clause: &'hir WhereClause) {
        walk_where_clause(self, clause)
    }

    fn visit_where_predicate(&mut self, predicate: &'hir WherePredicate) {
        walk_where_predicate(self, predicate)
    }

    fn visit_trait_bound(&mut self, bound: &'hir TraitBound) {
        walk_trait_bound(self, bound)
    }

    fn visit_trait_ref(&mut self, trait_ref: &'hir TraitRef) {
        walk_trait_ref(self, trait_ref)
    }

    fn visit_use_path(&mut self, path: &'hir UsePath) {
        walk_use_path(self, path)
    }

    // -------------------------------------------------------------------------
    // ID-based lookup helpers
    // -------------------------------------------------------------------------
    /// Visit an expression by its arena ID. The default implementation looks up
    /// the node in [`crate_hir`](Visitor::crate_hir) and calls [`visit_expr`].
    fn visit_expr_by_id(&mut self, id: ExprId) {
        if let Some(crate_hir) = self.crate_hir() {
            if let Some(expr) = crate_hir.exprs.get(id).and_then(|o| o.as_ref()) {
                self.visit_expr(expr);
            }
        }
    }

    /// Visit a pattern by its arena ID.
    fn visit_pat_by_id(&mut self, id: PatId) {
        if let Some(crate_hir) = self.crate_hir() {
            if let Some(pat) = crate_hir.pats.get(id).and_then(|o| o.as_ref()) {
                self.visit_pat(pat);
            }
        }
    }

    /// Visit a statement by its arena ID.
    fn visit_stmt_by_id(&mut self, id: StmtId) {
        if let Some(crate_hir) = self.crate_hir() {
            if let Some(stmt) = crate_hir.stmts.get(id).and_then(|o| o.as_ref()) {
                self.visit_stmt(stmt);
            }
        }
    }

    /// Visit a type by its arena ID.
    fn visit_ty_by_id(&mut self, id: HirTyId) {
        if let Some(crate_hir) = self.crate_hir() {
            if let Some(ty) = crate_hir.tys.get(id).and_then(|o| o.as_ref()) {
                self.visit_ty(ty);
            }
        }
    }

    /// Visit a body by its arena ID.
    fn visit_body_by_id(&mut self, id: BodyId) {
        if let Some(crate_hir) = self.crate_hir() {
            if let Some(body) = crate_hir.bodies.get(id).and_then(|o| o.as_ref()) {
                self.visit_body(body);
            }
        }
    }
}

pub fn walk_crate<'hir, V: Visitor<'hir>>(visitor: &mut V, crate_hir: &'hir Crate) {
    for item in crate_hir.items.values() {
        if let Some(item) = item {
            visitor.visit_item(item);
        }
    }
    for trait_ in crate_hir.traits.values() {
        if let Some(trait_) = trait_ {
            visitor.visit_trait(trait_);
        }
    }
    for impl_ in &crate_hir.impls {
        visitor.visit_impl(impl_);
    }
}

pub fn walk_item<'hir, V: Visitor<'hir>>(visitor: &mut V, item: &'hir Item) {
    let crate_hir = match visitor.crate_hir() {
        Some(c) => c,
        None => return,
    };
    match &item.kind {
        ItemKind::Fn { sig, body, generics } => {
            visitor.visit_generics(generics);
            walk_fn_sig(visitor, sig);
            visitor.visit_body_by_id(*body);
        }
        ItemKind::Struct { data, generics } => {
            visitor.visit_generics(generics);
            walk_variant_data(visitor, data);
        }
        ItemKind::Enum { def, generics } => {
            visitor.visit_generics(generics);
            for variant in &def.variants {
                visitor.visit_variant_def(variant);
            }
        }
        ItemKind::Trait {
            items,
            generics,
            super_traits,
        } => {
            visitor.visit_generics(generics);
            for super_trait in super_traits {
                visitor.visit_trait_ref(super_trait);
            }
            for trait_item in items {
                match &trait_item.kind {
                    crate::hir::core::TraitItemKind::Fn { sig, default } => {
                        walk_fn_sig(visitor, sig);
                        if let Some(body) = *default {
                            visitor.visit_body_by_id(body);
                        }
                    }
                    crate::hir::core::TraitItemKind::Const { ty, body } => {
                        visitor.visit_ty_by_id(*ty);
                        if let Some(body) = *body {
                            visitor.visit_body_by_id(body);
                        }
                    }
                    crate::hir::core::TraitItemKind::Type { bounds, default } => {
                        for bound in bounds {
                            visitor.visit_trait_bound(bound);
                        }
                        if let Some(ty) = default {
                            visitor.visit_ty_by_id(*ty);
                        }
                    }
                }
            }
        }
        ItemKind::Impl {
            items,
            generics,
            self_ty,
            of_trait,
            polarity: _,
        } => {
            visitor.visit_generics(generics);
            visitor.visit_ty_by_id(*self_ty);
            if let Some(trait_ref) = of_trait {
                visitor.visit_trait_ref(trait_ref);
            }
            for impl_item in items {
                match &impl_item.kind {
                    crate::hir::core::ImplItemKind::Fn { sig, body } => {
                        walk_fn_sig(visitor, sig);
                        visitor.visit_body_by_id(*body);
                    }
                    crate::hir::core::ImplItemKind::Const { ty, body } => {
                        visitor.visit_ty_by_id(*ty);
                        visitor.visit_body_by_id(*body);
                    }
                    crate::hir::core::ImplItemKind::Type { ty } => {
                        visitor.visit_ty_by_id(*ty);
                    }
                }
            }
        }
        ItemKind::TyAlias { ty, generics } => {
            visitor.visit_generics(generics);
            visitor.visit_ty_by_id(*ty);
        }
        ItemKind::Const { ty, body } => {
            visitor.visit_ty_by_id(*ty);
            visitor.visit_body_by_id(*body);
        }
        ItemKind::Static { ty, body, .. } => {
            visitor.visit_ty_by_id(*ty);
            visitor.visit_body_by_id(*body);
        }
        ItemKind::Mod { items } => {
            for def_id in items {
                if let Some(Some(item)) = crate_hir.items.get(*def_id) {
                    visitor.visit_item(item);
                }
            }
        }
        ItemKind::Use { path, .. } => {
            visitor.visit_use_path(path);
        }
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
        Expr::AssignOp { left, right, .. } => {
            visitor.visit_expr_by_id(*left);
            visitor.visit_expr_by_id(*right);
        }
        Expr::DestructureAssign { pat, value } => {
            visitor.visit_pat_by_id(*pat);
            visitor.visit_expr_by_id(*value);
        }
        Expr::Range { start, end, .. } => {
            if let Some(e) = start {
                visitor.visit_expr_by_id(*e);
            }
            if let Some(e) = end {
                visitor.visit_expr_by_id(*e);
            }
        }
        Expr::Object { fields } => {
            for field in fields {
                visitor.visit_expr_by_id(field.expr);
            }
        }
        Expr::IsType { expr: inner, ty } => {
            visitor.visit_expr_by_id(*inner);
            visitor.visit_ty_by_id(*ty);
        }
        Expr::TypeAscription { expr: inner, ty } => {
            visitor.visit_expr_by_id(*inner);
            visitor.visit_ty_by_id(*ty);
        }
        Expr::Try { expr: inner } | Expr::Await { expr: inner } => {
            visitor.visit_expr_by_id(*inner);
        }
        Expr::Async { body } | Expr::Gen { body, .. } => {
            visitor.visit_body_by_id(*body);
        }
        Expr::DocumentAccess { base, projection } => {
            visitor.visit_expr_by_id(*base);
            for proj in projection {
                match proj {
                    crate::hir::expr::DocumentProjection::Field { value, .. } => {
                        if let Some(e) = value {
                            visitor.visit_expr_by_id(*e);
                        }
                    }
                    crate::hir::expr::DocumentProjection::Spread(e) => visitor.visit_expr_by_id(*e),
                }
            }
        }
        Expr::Comprehension {
            element,
            variables,
            condition,
            ..
        } => {
            visitor.visit_expr_by_id(*element);
            for (pat, source) in variables {
                visitor.visit_pat_by_id(*pat);
                visitor.visit_expr_by_id(*source);
            }
            if let Some(cond) = condition {
                visitor.visit_expr_by_id(*cond);
            }
        }
        Expr::Lit { .. }
        | Expr::Path { .. }
        | Expr::Continue { .. }
        | Expr::Err => {}
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

pub fn walk_ty<'hir, V: Visitor<'hir>>(visitor: &mut V, ty: &'hir HirTy) {
    match ty {
        HirTy::Path { args, .. } => {
            for arg in args {
                walk_generic_arg(visitor, arg);
            }
        }
        HirTy::Tuple { tys } => {
            for t in tys {
                visitor.visit_ty_by_id(*t);
            }
        }
        HirTy::Array { ty: inner, len } => {
            visitor.visit_ty_by_id(*inner);
            walk_const(visitor, len);
        }
        HirTy::Slice { ty: inner } => {
            visitor.visit_ty_by_id(*inner);
        }
        HirTy::FnPtr { sig } => {
            walk_fn_sig(visitor, sig);
        }
        HirTy::AnonStruct { fields } => {
            for field in fields {
                visitor.visit_ty_by_id(field.ty);
            }
        }
        HirTy::TypeLit { .. } => {}
        HirTy::Utility { args, .. } => {
            for arg in args {
                visitor.visit_ty_by_id(*arg);
            }
        }
        HirTy::Ref { ty: inner, .. } | HirTy::RawPtr { ty: inner, .. } => {
            visitor.visit_ty_by_id(*inner);
        }
        HirTy::ForAll { params, ty: inner } => {
            for param in params {
                visitor.visit_binder_param(param);
            }
            visitor.visit_ty_by_id(*inner);
        }
        HirTy::Union { tys } => {
            for t in tys {
                visitor.visit_ty_by_id(*t);
            }
        }
        HirTy::ImplTrait { .. } | HirTy::DynTrait { .. } => {}
        HirTy::TypeOf { expr } => {
            visitor.visit_expr_by_id(*expr);
        }
        HirTy::Never | HirTy::Infer | HirTy::Missing | HirTy::Err => {}
    }
}

pub fn walk_generic_arg<'hir, V: Visitor<'hir>>(visitor: &mut V, arg: &'hir GenericArg) {
    match arg {
        GenericArg::Type(ty) => visitor.visit_ty_by_id(*ty),
        GenericArg::Const(c) => walk_const(visitor, c),
        GenericArg::AssocBinding { ty, .. } => visitor.visit_ty_by_id(*ty),
    }
}

pub fn walk_const<'hir, V: Visitor<'hir>>(visitor: &mut V, constant: &'hir Const) {
    match &constant.kind {
        ConstKind::Lit { .. } | ConstKind::Err => {}
        ConstKind::Expr { body } => visitor.visit_body_by_id(*body),
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
        Pat::Ref { pat, .. } => {
            visitor.visit_pat_by_id(*pat);
        }
        Pat::Rest { .. } | Pat::Wild | Pat::Path { .. } | Pat::Lit { .. } | Pat::Err => {}
    }
}

pub fn walk_fn_sig<'hir, V: Visitor<'hir>>(visitor: &mut V, sig: &'hir FnSig) {
    for ty in &sig.inputs {
        visitor.visit_ty_by_id(*ty);
    }
    visitor.visit_ty_by_id(sig.output);
}

pub fn walk_generics<'hir, V: Visitor<'hir>>(visitor: &mut V, generics: &'hir Generics) {
    for param in &generics.params {
        visitor.visit_generic_param(param);
    }
    if let Some(where_clause) = &generics.where_clause {
        visitor.visit_where_clause(where_clause);
    }
}

pub fn walk_generic_param<'hir, V: Visitor<'hir>>(visitor: &mut V, param: &'hir GenericParam) {
    match param {
        GenericParam::Type {
            bounds, default, ..
        } => {
            for bound in bounds {
                visitor.visit_trait_bound(bound);
            }
            if let Some(ty) = default {
                visitor.visit_ty_by_id(*ty);
            }
        }
        GenericParam::Const { ty, default, .. } => {
            visitor.visit_ty_by_id(*ty);
            if let Some(expr) = default {
                visitor.visit_expr_by_id(*expr);
            }
        }
    }
}

pub fn walk_binder_param<'hir, V: Visitor<'hir>>(visitor: &mut V, param: &'hir BinderParam) {
    match param {
        BinderParam::Type { bounds, .. } => {
            for bound in bounds {
                visitor.visit_trait_bound(bound);
            }
        }
        BinderParam::Const { ty, .. } => {
            visitor.visit_ty_by_id(*ty);
        }
    }
}

pub fn walk_where_clause<'hir, V: Visitor<'hir>>(visitor: &mut V, clause: &'hir WhereClause) {
    for predicate in &clause.predicates {
        visitor.visit_where_predicate(predicate);
    }
}

pub fn walk_where_predicate<'hir, V: Visitor<'hir>>(
    visitor: &mut V,
    predicate: &'hir WherePredicate,
) {
    match predicate {
        WherePredicate::TraitBound { ty, bounds } => {
            visitor.visit_ty_by_id(*ty);
            for bound in bounds {
                visitor.visit_trait_bound(bound);
            }
        }
        WherePredicate::TypeEq { lhs, rhs } => {
            visitor.visit_ty_by_id(*lhs);
            visitor.visit_ty_by_id(*rhs);
        }
    }
}

pub fn walk_trait_bound<'hir, V: Visitor<'hir>>(visitor: &mut V, bound: &'hir TraitBound) {
    for arg in &bound.args {
        walk_generic_arg(visitor, arg);
    }
}

pub fn walk_trait_ref<'hir, V: Visitor<'hir>>(_visitor: &mut V, _trait_ref: &'hir TraitRef) {
    // Trait references contain only a resolved path; no nested HIR nodes to walk.
}

pub fn walk_use_path<'hir, V: Visitor<'hir>>(visitor: &mut V, path: &'hir UsePath) {
    // Resolve the path to an item and visit it if possible.
    if let Some(crate_hir) = visitor.crate_hir() {
        let def_id = match path.res {
            Res::Def { def_id } => Some(def_id),
            Res::SelfTy { def_id } | Res::SelfVal { def_id } => Some(def_id),
            Res::Local { .. } | Res::PrimTy { .. } | Res::Err => None,
        };
        if let Some(def_id) = def_id {
            if let Some(Some(item)) = crate_hir.items.get(def_id) {
                visitor.visit_item(item);
            }
            if let Some(Some(trait_)) = crate_hir.traits.get(def_id) {
                visitor.visit_trait(trait_);
            }
        }
    }
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
    if let Some(discriminant) = &variant.discriminant {
        walk_const(visitor, discriminant);
    }
}

pub fn walk_field_def<'hir, V: Visitor<'hir>>(visitor: &mut V, field: &'hir FieldDef) {
    visitor.visit_ty_by_id(field.ty);
}

pub fn walk_struct_field<'hir, V: Visitor<'hir>>(visitor: &mut V, field: &'hir StructField) {
    visitor.visit_ty_by_id(field.ty);
}

pub fn walk_impl<'hir, V: Visitor<'hir>>(visitor: &mut V, impl_: &'hir Impl) {
    visitor.visit_generics(&impl_.generics);
    visitor.visit_ty_by_id(impl_.self_ty);
    if let Some(trait_ref) = &impl_.of_trait {
        visitor.visit_trait_ref(trait_ref);
    }
    for item in &impl_.items {
        visitor.visit_impl_item(item);
    }
}

pub fn walk_impl_item<'hir, V: Visitor<'hir>>(visitor: &mut V, item: &'hir ImplItem) {
    match &item.kind {
        crate::hir::core::ImplItemKind::Fn { sig, body } => {
            walk_fn_sig(visitor, sig);
            visitor.visit_body_by_id(*body);
        }
        crate::hir::core::ImplItemKind::Const { ty, body } => {
            visitor.visit_ty_by_id(*ty);
            visitor.visit_body_by_id(*body);
        }
        crate::hir::core::ImplItemKind::Type { ty } => {
            visitor.visit_ty_by_id(*ty);
        }
    }
}

pub fn walk_trait<'hir, V: Visitor<'hir>>(visitor: &mut V, trait_: &'hir Trait) {
    visitor.visit_generics(&trait_.generics);
    for super_trait in &trait_.super_traits {
        visitor.visit_trait_ref(super_trait);
    }
    for item in &trait_.items {
        visitor.visit_trait_item(item);
    }
}

pub fn walk_trait_item<'hir, V: Visitor<'hir>>(visitor: &mut V, item: &'hir TraitItem) {
    match &item.kind {
        crate::hir::core::TraitItemKind::Fn { sig, default } => {
            walk_fn_sig(visitor, sig);
            if let Some(body) = *default {
                visitor.visit_body_by_id(body);
            }
        }
        crate::hir::core::TraitItemKind::Const { ty, body } => {
            visitor.visit_ty_by_id(*ty);
            if let Some(body) = *body {
                visitor.visit_body_by_id(body);
            }
        }
        crate::hir::core::TraitItemKind::Type { bounds, default } => {
            for bound in bounds {
                visitor.visit_trait_bound(bound);
            }
            if let Some(ty) = default {
                visitor.visit_ty_by_id(*ty);
            }
        }
    }
}
