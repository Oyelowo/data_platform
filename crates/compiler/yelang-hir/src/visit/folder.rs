//! Functional HIR -> HIR rewrite via the [`Folder`] trait.
//!
//! A [`Folder`] receives owned HIR nodes and returns transformed nodes.  Nodes
//! referenced by arena IDs are transparently looked up, folded, and
//! re-allocated by the `fold_*_id` dispatch functions.
//!
//! The default [`Folder`] implementation is the identity transformation.  The
//! default `walk_*` helpers recursively fold children bottom-up: child IDs are
//! folded first, then the parent node is reconstructed with the new IDs and
//! passed to the trait method for final transformation.

use crate::crate_data::Crate;
use crate::hir::core::{
    Arm, BinderParam, Block, Expr, FieldDef, FnSig, GenericParam, Generics, Impl, ImplItem, Item,
    ItemKind, Stmt, StructField, Trait, TraitBound, TraitItem, TraitRef, Ty, UsePath, VariantData,
    VariantDef, WhereClause, WherePredicate,
};
use crate::hir::body::Body;
use crate::hir::pat::Pat;
use crate::hir::ty::{Const, ConstKind, GenericArg};
use crate::ids::{BodyId, DefId, ExprId, PatId, StmtId, HirTyId};
use crate::res::Res;

/// Functional HIR -> HIR rewrite.
///
/// Implementations override the methods for the node kinds they want to
/// transform.  The default implementations return the input unchanged; the
/// default `walk_*` helpers recursively fold children and reconstruct the
/// parent node.
pub trait Folder: Sized {
    fn fold_crate(&mut self, crate_hir: &mut Crate) {
        fold_crate(self, crate_hir);
    }

    fn fold_item(&mut self, item: Item) -> Item {
        item
    }

    fn fold_expr(&mut self, expr: Expr) -> Expr {
        expr
    }

    fn fold_stmt(&mut self, stmt: Stmt) -> Stmt {
        stmt
    }

    fn fold_ty(&mut self, ty: Ty) -> Ty {
        ty
    }

    fn fold_pat(&mut self, pat: Pat) -> Pat {
        pat
    }

    fn fold_body(&mut self, body: Body) -> Body {
        body
    }

    fn fold_block(&mut self, block: Block) -> Block {
        block
    }

    fn fold_arm(&mut self, arm: Arm) -> Arm {
        arm
    }

    fn fold_impl(&mut self, impl_: Impl) -> Impl {
        impl_
    }

    fn fold_impl_item(&mut self, item: ImplItem) -> ImplItem {
        item
    }

    fn fold_trait(&mut self, trait_: Trait) -> Trait {
        trait_
    }

    fn fold_trait_item(&mut self, item: TraitItem) -> TraitItem {
        item
    }

    fn fold_variant_def(&mut self, variant: VariantDef) -> VariantDef {
        variant
    }

    fn fold_field_def(&mut self, field: FieldDef) -> FieldDef {
        field
    }

    fn fold_struct_field(&mut self, field: StructField) -> StructField {
        field
    }

    fn fold_generics(&mut self, generics: Generics) -> Generics {
        generics
    }

    fn fold_generic_param(&mut self, param: GenericParam) -> GenericParam {
        param
    }

    fn fold_binder_param(&mut self, param: BinderParam) -> BinderParam {
        param
    }

    fn fold_where_clause(&mut self, clause: WhereClause) -> WhereClause {
        clause
    }

    fn fold_where_predicate(&mut self, predicate: WherePredicate) -> WherePredicate {
        predicate
    }

    fn fold_trait_bound(&mut self, bound: TraitBound) -> TraitBound {
        bound
    }

    fn fold_trait_ref(&mut self, trait_ref: TraitRef) -> TraitRef {
        trait_ref
    }

    fn fold_use_path(&mut self, path: UsePath) -> UsePath {
        path
    }
}

/// Fold an entire crate in place.
pub fn fold_crate(f: &mut impl Folder, crate_hir: &mut Crate) {
    let item_keys: Vec<_> = crate_hir.items.keys().collect();
    for def_id in item_keys {
        fold_item_id(f, crate_hir, def_id);
    }
    let trait_keys: Vec<_> = crate_hir.traits.keys().collect();
    for def_id in trait_keys {
        fold_trait_id(f, crate_hir, def_id);
    }
    let impls = std::mem::take(&mut crate_hir.impls);
    crate_hir.impls = impls.into_iter().map(|impl_| f.fold_impl(impl_)).collect();
}

/// Fold the item stored at `def_id` in place and return the same `DefId`.
pub fn fold_item_id(f: &mut impl Folder, crate_hir: &mut Crate, def_id: DefId) -> DefId {
    if let Some(item) = crate_hir.items.get_mut(def_id).and_then(|o| std::mem::take(o)) {
        // The walked item has all child IDs folded.
        let walked = walk_item(f, crate_hir, item);
        crate_hir.items[def_id] = Some(f.fold_item(walked));
    }
    def_id
}

/// Fold the trait definition stored at `def_id` in place and return the same
/// `DefId`.
pub fn fold_trait_id(f: &mut impl Folder, crate_hir: &mut Crate, def_id: DefId) -> DefId {
    if let Some(trait_) = crate_hir.traits.get_mut(def_id).and_then(|o| std::mem::take(o)) {
        let walked = walk_trait(f, crate_hir, trait_);
        crate_hir.traits[def_id] = Some(f.fold_trait(walked));
    }
    def_id
}

/// Fold the expression at `expr_id`, allocating the result in the arena.
///
/// The folded expression is always re-allocated even if no child changed.
/// Equality-based short-circuiting can be added later if the HIR types gain
/// `PartialEq`.
pub fn fold_expr_id(f: &mut impl Folder, crate_hir: &mut Crate, expr_id: ExprId) -> ExprId {
    let expr = match crate_hir.exprs.get(expr_id) {
        Some(slot) => slot.clone(),
        None => return expr_id,
    };
    let Some(expr) = expr else { return expr_id };
    let walked = walk_expr(f, crate_hir, expr);
    let folded = f.fold_expr(walked);
    let span = crate_hir.expr_span(expr_id);
    crate_hir.alloc_expr(folded, span)
}

/// Fold the statement at `stmt_id`, allocating the result in the arena.
pub fn fold_stmt_id(f: &mut impl Folder, crate_hir: &mut Crate, stmt_id: StmtId) -> StmtId {
    let stmt = match crate_hir.stmts.get(stmt_id) {
        Some(slot) => slot.clone(),
        None => return stmt_id,
    };
    let Some(stmt) = stmt else { return stmt_id };
    let walked = walk_stmt(f, crate_hir, stmt);
    let folded = f.fold_stmt(walked);
    let span = crate_hir.stmt_span(stmt_id);
    crate_hir.alloc_stmt(folded, span)
}

/// Fold the type at `ty_id`, allocating the result in the arena.
pub fn fold_ty_id(f: &mut impl Folder, crate_hir: &mut Crate, ty_id: HirTyId) -> HirTyId {
    let ty = match crate_hir.tys.get(ty_id) {
        Some(slot) => slot.clone(),
        None => return ty_id,
    };
    let Some(ty) = ty else { return ty_id };
    let walked = walk_ty(f, crate_hir, ty);
    let folded = f.fold_ty(walked);
    let span = crate_hir.ty_span(ty_id);
    crate_hir.alloc_ty(folded, span)
}

/// Fold the pattern at `pat_id`, allocating the result in the arena.
pub fn fold_pat_id(f: &mut impl Folder, crate_hir: &mut Crate, pat_id: PatId) -> PatId {
    let pat = match crate_hir.pats.get(pat_id) {
        Some(slot) => slot.clone(),
        None => return pat_id,
    };
    let Some(pat) = pat else { return pat_id };
    let walked = walk_pat(f, crate_hir, pat);
    let folded = f.fold_pat(walked);
    let span = crate_hir.pat_span(pat_id);
    crate_hir.alloc_pat(folded, span)
}

/// Fold the body at `body_id`, allocating the result in the arena.
pub fn fold_body_id(f: &mut impl Folder, crate_hir: &mut Crate, body_id: BodyId) -> BodyId {
    let body = match crate_hir.bodies.get(body_id) {
        Some(slot) => slot.clone(),
        None => return body_id,
    };
    let Some(body) = body else { return body_id };
    let walked = walk_body(f, crate_hir, body);
    let folded = f.fold_body(walked);
    let span = crate_hir.body_span(body_id);
    crate_hir.alloc_body(folded, span)
}

pub fn walk_item(f: &mut impl Folder, crate_hir: &mut Crate, item: Item) -> Item {
    let new_kind = match item.kind {
        ItemKind::Fn { sig, body, generics } => ItemKind::Fn {
            sig: walk_fn_sig(f, crate_hir, sig),
            body: fold_body_id(f, crate_hir, body),
            generics: walk_generics(f, crate_hir, generics),
        },
        ItemKind::Struct { data, generics } => ItemKind::Struct {
            data: walk_variant_data(f, crate_hir, data),
            generics: walk_generics(f, crate_hir, generics),
        },
        ItemKind::Enum { def, generics } => ItemKind::Enum {
            def: crate::hir::core::EnumDef {
                variants: def
                    .variants
                    .into_iter()
                    .map(|variant| walk_variant_def(f, crate_hir, variant))
                    .collect(),
                span: def.span,
            },
            generics: walk_generics(f, crate_hir, generics),
        },
        ItemKind::Trait {
            items,
            generics,
            super_traits,
        } => ItemKind::Trait {
            items: items
                .into_iter()
                .map(|item| walk_trait_item(f, crate_hir, item))
                .collect(),
            generics: walk_generics(f, crate_hir, generics),
            super_traits: super_traits
                .into_iter()
                .map(|trait_ref| f.fold_trait_ref(trait_ref))
                .collect(),
        },
        ItemKind::Impl {
            items,
            generics,
            self_ty,
            of_trait,
            polarity,
        } => ItemKind::Impl {
            items: items
                .into_iter()
                .map(|item| walk_impl_item(f, crate_hir, item))
                .collect(),
            generics: walk_generics(f, crate_hir, generics),
            self_ty: fold_ty_id(f, crate_hir, self_ty),
            of_trait: of_trait.map(|trait_ref| f.fold_trait_ref(trait_ref)),
            polarity,
        },
        ItemKind::TyAlias { ty, generics } => ItemKind::TyAlias {
            ty: fold_ty_id(f, crate_hir, ty),
            generics: walk_generics(f, crate_hir, generics),
        },
        ItemKind::Const { ty, body } => ItemKind::Const {
            ty: fold_ty_id(f, crate_hir, ty),
            body: fold_body_id(f, crate_hir, body),
        },
        ItemKind::Static {
            ty,
            mutability,
            body,
        } => ItemKind::Static {
            ty: fold_ty_id(f, crate_hir, ty),
            mutability,
            body: fold_body_id(f, crate_hir, body),
        },
        ItemKind::Mod { items } => ItemKind::Mod {
            items: items
                .into_iter()
                .map(|def_id| fold_item_id(f, crate_hir, def_id))
                .collect(),
        },
        ItemKind::Use { path, kind } => ItemKind::Use {
            path: walk_use_path(f, crate_hir, path),
            kind,
        },
    };
    Item {
        def_id: item.def_id,
        ident: item.ident,
        kind: new_kind,
        vis: item.vis,
        attrs: item.attrs,
        span: item.span,
    }
}

pub fn walk_expr(f: &mut impl Folder, crate_hir: &mut Crate, expr: Expr) -> Expr {
    match expr {
        Expr::Binary { op, left, right } => Expr::Binary {
            op,
            left: fold_expr_id(f, crate_hir, left),
            right: fold_expr_id(f, crate_hir, right),
        },
        Expr::Unary { op, expr: inner } => Expr::Unary {
            op,
            expr: fold_expr_id(f, crate_hir, inner),
        },
        Expr::Call { func, args } => Expr::Call {
            func: fold_expr_id(f, crate_hir, func),
            args: args
                .into_iter()
                .map(|arg| fold_expr_id(f, crate_hir, arg))
                .collect(),
        },
        Expr::MethodCall {
            receiver,
            method,
            args,
            trait_def_id,
        } => Expr::MethodCall {
            receiver: fold_expr_id(f, crate_hir, receiver),
            method,
            args: args
                .into_iter()
                .map(|arg| fold_expr_id(f, crate_hir, arg))
                .collect(),
            trait_def_id,
        },
        Expr::Field { expr: inner, field } => Expr::Field {
            expr: fold_expr_id(f, crate_hir, inner),
            field,
        },
        Expr::Index { expr: inner, index } => Expr::Index {
            expr: fold_expr_id(f, crate_hir, inner),
            index: fold_expr_id(f, crate_hir, index),
        },
        Expr::Assign { left, right } => Expr::Assign {
            left: fold_expr_id(f, crate_hir, left),
            right: fold_expr_id(f, crate_hir, right),
        },
        Expr::Block { block } => Expr::Block {
            block: walk_block(f, crate_hir, block),
        },
        Expr::Loop { block, label } => Expr::Loop {
            block: walk_block(f, crate_hir, block),
            label,
        },
        Expr::Break { label, expr } => Expr::Break {
            label,
            expr: expr.map(|e| fold_expr_id(f, crate_hir, e)),
        },
        Expr::Return { expr } => Expr::Return {
            expr: expr.map(|e| fold_expr_id(f, crate_hir, e)),
        },
        Expr::Match { expr, arms } => Expr::Match {
            expr: fold_expr_id(f, crate_hir, expr),
            arms: arms
                .into_iter()
                .map(|arm| walk_arm(f, crate_hir, arm))
                .collect(),
        },
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => Expr::If {
            cond: fold_expr_id(f, crate_hir, cond),
            then_branch: fold_expr_id(f, crate_hir, then_branch),
            else_branch: else_branch.map(|e| fold_expr_id(f, crate_hir, e)),
        },
        Expr::Closure {
            params,
            body,
            capture_clause,
        } => Expr::Closure {
            params: params
                .into_iter()
                .map(|param| crate::hir::body::Param {
                    pat: fold_pat_id(f, crate_hir, param.pat),
                    ty: fold_ty_id(f, crate_hir, param.ty),
                    span: param.span,
                })
                .collect(),
            body: fold_body_id(f, crate_hir, body),
            capture_clause,
        },
        Expr::Struct { path, fields, rest } => Expr::Struct {
            path,
            fields: fields
                .into_iter()
                .map(|field| crate::hir::core::FieldExpr {
                    ident: field.ident,
                    expr: fold_expr_id(f, crate_hir, field.expr),
                    span: field.span,
                })
                .collect(),
            rest: rest.map(|e| fold_expr_id(f, crate_hir, e)),
        },
        Expr::Tuple { exprs } => Expr::Tuple {
            exprs: exprs
                .into_iter()
                .map(|e| fold_expr_id(f, crate_hir, e))
                .collect(),
        },
        Expr::Array { exprs } => Expr::Array {
            exprs: exprs
                .into_iter()
                .map(|e| fold_expr_id(f, crate_hir, e))
                .collect(),
        },
        Expr::Cast { expr: inner, ty } => Expr::Cast {
            expr: fold_expr_id(f, crate_hir, inner),
            ty: fold_ty_id(f, crate_hir, ty),
        },
        Expr::Let { pat, expr: inner } => Expr::Let {
            pat: fold_pat_id(f, crate_hir, pat),
            expr: fold_expr_id(f, crate_hir, inner),
        },
        Expr::AssignOp { op, left, right } => Expr::AssignOp {
            op,
            left: fold_expr_id(f, crate_hir, left),
            right: fold_expr_id(f, crate_hir, right),
        },
        Expr::DestructureAssign { pat, value } => Expr::DestructureAssign {
            pat: fold_pat_id(f, crate_hir, pat),
            value: fold_expr_id(f, crate_hir, value),
        },
        Expr::Range {
            start,
            end,
            inclusive,
        } => Expr::Range {
            start: start.map(|e| fold_expr_id(f, crate_hir, e)),
            end: end.map(|e| fold_expr_id(f, crate_hir, e)),
            inclusive,
        },
        Expr::Object { fields } => Expr::Object {
            fields: fields
                .into_iter()
                .map(|field| crate::hir::core::FieldExpr {
                    ident: field.ident,
                    expr: fold_expr_id(f, crate_hir, field.expr),
                    span: field.span,
                })
                .collect(),
        },
        Expr::IsType { expr: inner, ty } => Expr::IsType {
            expr: fold_expr_id(f, crate_hir, inner),
            ty: fold_ty_id(f, crate_hir, ty),
        },
        Expr::Try { expr: inner } => Expr::Try {
            expr: fold_expr_id(f, crate_hir, inner),
        },
        Expr::Await { expr: inner } => Expr::Await {
            expr: fold_expr_id(f, crate_hir, inner),
        },
        Expr::Async { body } => Expr::Async {
            body: fold_body_id(f, crate_hir, body),
        },
        Expr::Gen { kind, body } => Expr::Gen {
            kind,
            body: fold_body_id(f, crate_hir, body),
        },
        Expr::TypeAscription { expr: inner, ty } => Expr::TypeAscription {
            expr: fold_expr_id(f, crate_hir, inner),
            ty: fold_ty_id(f, crate_hir, ty),
        },
        Expr::DocumentAccess { base, projection } => Expr::DocumentAccess {
            base: fold_expr_id(f, crate_hir, base),
            projection: projection
                .into_iter()
                .map(|proj| match proj {
                    crate::hir::expr::DocumentProjection::Field { name, value } => {
                        crate::hir::expr::DocumentProjection::Field {
                            name,
                            value: value.map(|e| fold_expr_id(f, crate_hir, e)),
                        }
                    }
                    crate::hir::expr::DocumentProjection::Spread(e) => {
                        crate::hir::expr::DocumentProjection::Spread(fold_expr_id(f, crate_hir, e))
                    }
                })
                .collect(),
        },
        Expr::Comprehension {
            kind,
            element,
            variables,
            condition,
        } => Expr::Comprehension {
            kind,
            element: fold_expr_id(f, crate_hir, element),
            variables: variables
                .into_iter()
                .map(|(pat, source)| {
                    (
                        fold_pat_id(f, crate_hir, pat),
                        fold_expr_id(f, crate_hir, source),
                    )
                })
                .collect(),
            condition: condition.map(|e| fold_expr_id(f, crate_hir, e)),
        },
        Expr::Lit { .. } | Expr::Path { .. } | Expr::Continue { .. } | Expr::Err => expr,
    }
}

pub fn walk_stmt(f: &mut impl Folder, crate_hir: &mut Crate, stmt: Stmt) -> Stmt {
    match stmt {
        Stmt::Expr { expr } => Stmt::Expr {
            expr: fold_expr_id(f, crate_hir, expr),
        },
        Stmt::Let { pat, ty, init } => Stmt::Let {
            pat: fold_pat_id(f, crate_hir, pat),
            ty: ty.map(|t| fold_ty_id(f, crate_hir, t)),
            init: init.map(|e| fold_expr_id(f, crate_hir, e)),
        },
        Stmt::Item { item } => {
            let walked = walk_item(f, crate_hir, item);
            Stmt::Item {
                item: f.fold_item(walked),
            }
        }
    }
}

pub fn walk_block(f: &mut impl Folder, crate_hir: &mut Crate, block: Block) -> Block {
    Block {
        stmts: block
            .stmts
            .into_iter()
            .map(|stmt| fold_stmt_id(f, crate_hir, stmt))
            .collect(),
        expr: block.expr.map(|e| fold_expr_id(f, crate_hir, e)),
        span: block.span,
    }
}

pub fn walk_arm(f: &mut impl Folder, crate_hir: &mut Crate, arm: Arm) -> Arm {
    Arm {
        pat: fold_pat_id(f, crate_hir, arm.pat),
        guard: arm.guard.map(|g| fold_expr_id(f, crate_hir, g)),
        body: fold_expr_id(f, crate_hir, arm.body),
        span: arm.span,
    }
}

pub fn walk_body(f: &mut impl Folder, crate_hir: &mut Crate, body: Body) -> Body {
    Body {
        params: body
            .params
            .into_iter()
            .map(|param| crate::hir::body::Param {
                pat: fold_pat_id(f, crate_hir, param.pat),
                ty: fold_ty_id(f, crate_hir, param.ty),
                span: param.span,
            })
            .collect(),
        value: fold_expr_id(f, crate_hir, body.value),
        span: body.span,
    }
}

pub fn walk_ty(f: &mut impl Folder, crate_hir: &mut Crate, ty: Ty) -> Ty {
    match ty {
        Ty::Path { res, args } => Ty::Path {
            res,
            args: args
                .into_iter()
                .map(|arg| walk_generic_arg(f, crate_hir, arg))
                .collect(),
        },
        Ty::Tuple { tys } => Ty::Tuple {
            tys: tys
                .into_iter()
                .map(|t| fold_ty_id(f, crate_hir, t))
                .collect(),
        },
        Ty::Array { ty: inner, len } => Ty::Array {
            ty: fold_ty_id(f, crate_hir, inner),
            len: walk_const(f, crate_hir, len),
        },
        Ty::Slice { ty: inner } => Ty::Slice {
            ty: fold_ty_id(f, crate_hir, inner),
        },
        Ty::FnPtr { sig } => Ty::FnPtr {
            sig: Box::new(walk_fn_sig(f, crate_hir, *sig)),
        },
        Ty::AnonStruct { fields } => Ty::AnonStruct {
            fields: fields
                .into_iter()
                .map(|field| crate::hir::ty::AnonField {
                    name: field.name,
                    ty: fold_ty_id(f, crate_hir, field.ty),
                })
                .collect(),
        },
        Ty::TypeLit { variants } => Ty::TypeLit { variants },
        Ty::Utility { kind, args } => Ty::Utility {
            kind,
            args: args
                .into_iter()
                .map(|arg| fold_ty_id(f, crate_hir, arg))
                .collect(),
        },
        Ty::TypeOf { expr } => Ty::TypeOf {
            expr: fold_expr_id(f, crate_hir, expr),
        },
        Ty::Ref { mutability, ty: inner } => Ty::Ref {
            mutability,
            ty: fold_ty_id(f, crate_hir, inner),
        },
        Ty::RawPtr { mutability, ty: inner } => Ty::RawPtr {
            mutability,
            ty: fold_ty_id(f, crate_hir, inner),
        },
        Ty::ForAll { params, ty: inner } => Ty::ForAll {
            params: params
                .into_iter()
                .map(|param| walk_binder_param(f, crate_hir, param))
                .collect(),
            ty: fold_ty_id(f, crate_hir, inner),
        },
        Ty::Union { tys } => Ty::Union {
            tys: tys
                .into_iter()
                .map(|t| fold_ty_id(f, crate_hir, t))
                .collect(),
        },
        Ty::ImplTrait { path } => Ty::ImplTrait { path },
        Ty::DynTrait { path } => Ty::DynTrait { path },
        Ty::Never | Ty::Infer | Ty::Missing | Ty::Err => ty,
    }
}

pub fn walk_generic_arg(f: &mut impl Folder, crate_hir: &mut Crate, arg: GenericArg) -> GenericArg {
    match arg {
        GenericArg::Type(ty) => GenericArg::Type(fold_ty_id(f, crate_hir, ty)),
        GenericArg::Const(c) => GenericArg::Const(walk_const(f, crate_hir, c)),
        GenericArg::AssocBinding { name, ty } => GenericArg::AssocBinding {
            name,
            ty: fold_ty_id(f, crate_hir, ty),
        },
    }
}

pub fn walk_const(f: &mut impl Folder, crate_hir: &mut Crate, constant: Const) -> Const {
    Const {
        kind: match constant.kind {
            ConstKind::Lit { lit } => ConstKind::Lit { lit },
            ConstKind::Expr { body } => ConstKind::Expr {
                body: fold_body_id(f, crate_hir, body),
            },
            ConstKind::Err => ConstKind::Err,
        },
        span: constant.span,
    }
}

pub fn walk_pat(f: &mut impl Folder, crate_hir: &mut Crate, pat: Pat) -> Pat {
    match pat {
        Pat::Wild => Pat::Wild,
        Pat::Binding { mode, name, subpat } => Pat::Binding {
            mode,
            name,
            subpat: subpat.map(|p| fold_pat_id(f, crate_hir, p)),
        },
        Pat::Struct { res, fields, rest } => Pat::Struct {
            res,
            fields: fields
                .into_iter()
                .map(|field| crate::hir::pat::FieldPat {
                    ident: field.ident,
                    pat: fold_pat_id(f, crate_hir, field.pat),
                    is_shorthand: field.is_shorthand,
                    span: field.span,
                })
                .collect(),
            rest,
        },
        Pat::Tuple { pats } => Pat::Tuple {
            pats: pats
                .into_iter()
                .map(|p| fold_pat_id(f, crate_hir, p))
                .collect(),
        },
        Pat::TupleStruct { res, pats } => Pat::TupleStruct {
            res,
            pats: pats
                .into_iter()
                .map(|p| fold_pat_id(f, crate_hir, p))
                .collect(),
        },
        Pat::Ref { pat, mutability } => Pat::Ref {
            pat: fold_pat_id(f, crate_hir, pat),
            mutability,
        },
        Pat::Path { res } => Pat::Path { res },
        Pat::Lit { lit } => Pat::Lit { lit },
        Pat::Range {
            start,
            end,
            end_inclusive,
        } => Pat::Range {
            start: start.map(|p| fold_pat_id(f, crate_hir, p)),
            end: end.map(|p| fold_pat_id(f, crate_hir, p)),
            end_inclusive,
        },
        Pat::Or { pats } => Pat::Or {
            pats: pats
                .into_iter()
                .map(|p| fold_pat_id(f, crate_hir, p))
                .collect(),
        },
        Pat::Slice {
            prefix,
            middle,
            suffix,
        } => Pat::Slice {
            prefix: prefix
                .into_iter()
                .map(|p| fold_pat_id(f, crate_hir, p))
                .collect(),
            middle: middle.map(|p| fold_pat_id(f, crate_hir, p)),
            suffix: suffix
                .into_iter()
                .map(|p| fold_pat_id(f, crate_hir, p))
                .collect(),
        },
        Pat::Rest { name } => Pat::Rest { name },
        Pat::Err => Pat::Err,
    }
}

pub fn walk_fn_sig(f: &mut impl Folder, crate_hir: &mut Crate, sig: FnSig) -> FnSig {
    FnSig {
        inputs: sig
            .inputs
            .into_iter()
            .map(|ty| fold_ty_id(f, crate_hir, ty))
            .collect(),
        output: fold_ty_id(f, crate_hir, sig.output),
        is_async: sig.is_async,
        is_const: sig.is_const,
        is_variadic: sig.is_variadic,
        abi: sig.abi,
        bound_vars: sig.bound_vars,
    }
}

pub fn walk_generics(f: &mut impl Folder, crate_hir: &mut Crate, generics: Generics) -> Generics {
    Generics {
        params: generics
            .params
            .into_iter()
            .map(|param| walk_generic_param(f, crate_hir, param))
            .collect(),
        where_clause: generics
            .where_clause
            .map(|clause| walk_where_clause(f, crate_hir, clause)),
        span: generics.span,
    }
}

pub fn walk_generic_param(
    f: &mut impl Folder,
    crate_hir: &mut Crate,
    param: GenericParam,
) -> GenericParam {
    match param {
        GenericParam::Type {
            def_id,
            name,
            bounds,
            default,
            span,
        } => GenericParam::Type {
            def_id,
            name,
            bounds: bounds
                .into_iter()
                .map(|bound| f.fold_trait_bound(bound))
                .collect(),
            default: default.map(|ty| fold_ty_id(f, crate_hir, ty)),
            span,
        },
        GenericParam::Const {
            def_id,
            name,
            ty,
            default,
            span,
        } => GenericParam::Const {
            def_id,
            name,
            ty: fold_ty_id(f, crate_hir, ty),
            default: default.map(|expr| fold_expr_id(f, crate_hir, expr)),
            span,
        },
    }
}

pub fn walk_binder_param(
    f: &mut impl Folder,
    crate_hir: &mut Crate,
    param: BinderParam,
) -> BinderParam {
    match param {
        BinderParam::Type {
            name,
            bounds,
            span,
        } => BinderParam::Type {
            name,
            bounds: bounds
                .into_iter()
                .map(|bound| f.fold_trait_bound(bound))
                .collect(),
            span,
        },
        BinderParam::Const { name, ty, span } => BinderParam::Const {
            name,
            ty: fold_ty_id(f, crate_hir, ty),
            span,
        },
    }
}

pub fn walk_where_clause(
    f: &mut impl Folder,
    crate_hir: &mut Crate,
    clause: WhereClause,
) -> WhereClause {
    WhereClause {
        predicates: clause
            .predicates
            .into_iter()
            .map(|predicate| walk_where_predicate(f, crate_hir, predicate))
            .collect(),
        span: clause.span,
    }
}

pub fn walk_where_predicate(
    f: &mut impl Folder,
    crate_hir: &mut Crate,
    predicate: WherePredicate,
) -> WherePredicate {
    match predicate {
        WherePredicate::TraitBound { ty, bounds } => WherePredicate::TraitBound {
            ty: fold_ty_id(f, crate_hir, ty),
            bounds: bounds
                .into_iter()
                .map(|bound| f.fold_trait_bound(bound))
                .collect(),
        },
        WherePredicate::TypeEq { lhs, rhs } => WherePredicate::TypeEq {
            lhs: fold_ty_id(f, crate_hir, lhs),
            rhs: fold_ty_id(f, crate_hir, rhs),
        },
    }
}

pub fn walk_trait_bound(f: &mut impl Folder, crate_hir: &mut Crate, mut bound: TraitBound) -> TraitBound {
    bound.args = bound
        .args
        .into_iter()
        .map(|arg| walk_generic_arg(f, crate_hir, arg))
        .collect();
    f.fold_trait_bound(bound)
}

pub fn walk_trait_ref(
    f: &mut impl Folder,
    crate_hir: &mut Crate,
    trait_ref: TraitRef,
) -> TraitRef {
    let _ = crate_hir;
    f.fold_trait_ref(trait_ref)
}

pub fn walk_use_path(f: &mut impl Folder, crate_hir: &mut Crate, path: UsePath) -> UsePath {
    let def_id = match path.res {
        Res::Def { def_id } => Some(def_id),
        Res::SelfTy { def_id } | Res::SelfVal { def_id } => Some(def_id),
        Res::Local { .. } | Res::PrimTy { .. } | Res::Err => None,
    };
    if let Some(def_id) = def_id {
        fold_item_id(f, crate_hir, def_id);
        fold_trait_id(f, crate_hir, def_id);
    }
    f.fold_use_path(path)
}

pub fn walk_variant_data(
    f: &mut impl Folder,
    crate_hir: &mut Crate,
    data: VariantData,
) -> VariantData {
    let _ = crate_hir;
    match data {
        VariantData::Struct { fields } => VariantData::Struct {
            fields: fields
                .into_iter()
                .map(|field| f.fold_field_def(field))
                .collect(),
        },
        VariantData::Tuple { fields } => VariantData::Tuple {
            fields: fields
                .into_iter()
                .map(|field| f.fold_struct_field(field))
                .collect(),
        },
        VariantData::Unit => VariantData::Unit,
    }
}

pub fn walk_variant_def(
    f: &mut impl Folder,
    crate_hir: &mut Crate,
    variant: VariantDef,
) -> VariantDef {
    VariantDef {
        def_id: variant.def_id,
        ident: variant.ident,
        data: walk_variant_data(f, crate_hir, variant.data),
        discriminant: variant.discriminant.map(|c| walk_const(f, crate_hir, c)),
        attrs: variant.attrs,
        span: variant.span,
    }
}

pub fn walk_field_def(f: &mut impl Folder, crate_hir: &mut Crate, field: FieldDef) -> FieldDef {
    FieldDef {
        def_id: field.def_id,
        ident: field.ident,
        ty: fold_ty_id(f, crate_hir, field.ty),
        span: field.span,
        vis: field.vis,
        attrs: field.attrs,
    }
}

pub fn walk_struct_field(
    f: &mut impl Folder,
    crate_hir: &mut Crate,
    field: StructField,
) -> StructField {
    StructField {
        def_id: field.def_id,
        ty: fold_ty_id(f, crate_hir, field.ty),
        span: field.span,
        vis: field.vis,
        attrs: field.attrs,
    }
}

pub fn walk_impl(f: &mut impl Folder, crate_hir: &mut Crate, impl_: Impl) -> Impl {
    Impl {
        def_id: impl_.def_id,
        generics: walk_generics(f, crate_hir, impl_.generics),
        self_ty: fold_ty_id(f, crate_hir, impl_.self_ty),
        of_trait: impl_.of_trait.map(|tr| f.fold_trait_ref(tr)),
        items: impl_
            .items
            .into_iter()
            .map(|item| walk_impl_item(f, crate_hir, item))
            .collect(),
        polarity: impl_.polarity,
        span: impl_.span,
    }
}

pub fn walk_impl_item(f: &mut impl Folder, crate_hir: &mut Crate, item: ImplItem) -> ImplItem {
    let new_kind = match item.kind {
        crate::hir::core::ImplItemKind::Fn { sig, body } => {
            crate::hir::core::ImplItemKind::Fn {
                sig: walk_fn_sig(f, crate_hir, sig),
                body: fold_body_id(f, crate_hir, body),
            }
        }
        crate::hir::core::ImplItemKind::Const { ty, body } => {
            crate::hir::core::ImplItemKind::Const {
                ty: fold_ty_id(f, crate_hir, ty),
                body: fold_body_id(f, crate_hir, body),
            }
        }
        crate::hir::core::ImplItemKind::Type { ty } => {
            crate::hir::core::ImplItemKind::Type {
                ty: fold_ty_id(f, crate_hir, ty),
            }
        }
    };
    ImplItem {
        def_id: item.def_id,
        ident: item.ident,
        kind: new_kind,
        attrs: item.attrs,
        span: item.span,
        defaultness: item.defaultness,
    }
}

pub fn walk_trait(f: &mut impl Folder, crate_hir: &mut Crate, trait_: Trait) -> Trait {
    Trait {
        name: trait_.name,
        generics: walk_generics(f, crate_hir, trait_.generics),
        super_traits: trait_
            .super_traits
            .into_iter()
            .map(|tr| f.fold_trait_ref(tr))
            .collect(),
        items: trait_
            .items
            .into_iter()
            .map(|item| walk_trait_item(f, crate_hir, item))
            .collect(),
        span: trait_.span,
    }
}

pub fn walk_trait_item(f: &mut impl Folder, crate_hir: &mut Crate, item: TraitItem) -> TraitItem {
    let new_kind = match item.kind {
        crate::hir::core::TraitItemKind::Fn { sig, default } => {
            crate::hir::core::TraitItemKind::Fn {
                sig: walk_fn_sig(f, crate_hir, sig),
                default: default.map(|body| fold_body_id(f, crate_hir, body)),
            }
        }
        crate::hir::core::TraitItemKind::Const { ty, body } => {
            crate::hir::core::TraitItemKind::Const {
                ty: fold_ty_id(f, crate_hir, ty),
                body: body.map(|b| fold_body_id(f, crate_hir, b)),
            }
        }
        crate::hir::core::TraitItemKind::Type { bounds, default } => {
            crate::hir::core::TraitItemKind::Type {
                bounds: bounds
                    .into_iter()
                    .map(|bound| f.fold_trait_bound(bound))
                    .collect(),
                default: default.map(|ty| fold_ty_id(f, crate_hir, ty)),
            }
        }
    };
    TraitItem {
        def_id: item.def_id,
        ident: item.ident,
        kind: new_kind,
        attrs: item.attrs,
        span: item.span,
    }
}
