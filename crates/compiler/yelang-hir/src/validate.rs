//! HIR validation pass.
//!
//! Checks structural invariants of the HIR without panicking on malformed input.
//! Implemented as a HIR visitor so it stays in sync with the tree shape.

use std::collections::HashSet;

use yelang_interner::Symbol;
use yelang_lexer::Span;

use crate::hir::core::{
    Expr, ImplItem, ImplItemKind, Item, ItemKind, Pat, TraitBound, TraitItem, TraitItemKind,
    TraitRef, Ty, UsePath,
};
use crate::crate_data::Crate;
use crate::ids::{BodyId, ExprId, PatId, StmtId, TyId};
use crate::res::Res;
use crate::hir::ty::Const;
use crate::visit::visitor::{Visitor, walk_crate, walk_expr, walk_item, walk_pat, walk_ty};

/// An error reported by the HIR validation pass.
#[derive(Debug, Clone)]
pub struct ValidationError {
    pub message: String,
    pub span: Option<Span>,
}

/// Validate the structural invariants of `crate_hir`.
///
/// Returns a vector of errors; an empty vector means no violations were found.
/// The validator never panics on invalid IDs — it records an error and continues.
pub fn validate_hir(crate_hir: &Crate) -> Vec<ValidationError> {
    let mut validator = Validator::new(crate_hir);
    validator.validate_crate();
    validator.errors
}

struct Validator<'hir> {
    crate_hir: &'hir Crate,
    errors: Vec<ValidationError>,
    /// Number of enclosing function bodies (used to check `Return`).
    function_depth: usize,
    /// Stack of labels for in-scope loops. `None` means an unlabeled loop.
    loop_labels: Vec<Option<Symbol>>,
    /// Stack of binding-name sets, one per function body.
    bindings: Vec<HashSet<Symbol>>,
}

impl<'hir> Validator<'hir> {
    fn new(crate_hir: &'hir Crate) -> Self {
        Self {
            crate_hir,
            errors: Vec::new(),
            function_depth: 0,
            loop_labels: Vec::new(),
            bindings: Vec::new(),
        }
    }

    fn error(&mut self, message: impl Into<String>, span: Option<Span>) {
        self.errors.push(ValidationError {
            message: message.into(),
            span,
        });
    }

    fn in_function(&self) -> bool {
        self.function_depth > 0
    }

    fn with_function<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.function_depth += 1;
        self.bindings.push(HashSet::new());
        let result = f(self);
        self.bindings.pop();
        self.function_depth -= 1;
        result
    }

    fn with_loop_label<R>(&mut self, label: Option<Symbol>, f: impl FnOnce(&mut Self) -> R) -> R {
        self.loop_labels.push(label);
        let result = f(self);
        self.loop_labels.pop();
        result
    }

    fn check_binding(&mut self, name: Symbol, span: Option<Span>) {
        if let Some(set) = self.bindings.last_mut() {
            if !set.insert(name) {
                self.error(
                    format!("Duplicate binding name '{}' in function body", name),
                    span,
                );
            }
        }
    }

    fn check_def_id(&mut self, def_id: crate::ids::DefId, span: Option<Span>) {
        let in_items = self.crate_hir.items.get(def_id).is_some();
        let in_foreign = self.crate_hir.foreign_items.get(def_id).is_some();
        if !in_items && !in_foreign {
            self.error(
                format!(
                    "DefId {} is not present in Crate::items or Crate::foreign_items",
                    def_id
                ),
                span,
            );
        }
    }

    fn check_res(&mut self, res: &Res, span: Option<Span>) {
        match res {
            Res::Def { def_id } | Res::SelfTy { def_id } | Res::SelfVal { def_id } => {
                self.check_def_id(*def_id, span);
            }
            Res::Local { pat_id } => {
                if self.crate_hir.pats.get(*pat_id).is_none() {
                    self.error("PatId is not allocated", span);
                }
            }
            Res::PrimTy { .. } | Res::Err => {}
        }
    }

    fn check_const(&mut self, konst: &Const, _span: Option<Span>) {
        match &konst.kind {
            crate::hir::ty::ConstKind::Lit { .. } | crate::hir::ty::ConstKind::Err => {}
            crate::hir::ty::ConstKind::Expr { body } => {
                self.visit_body_by_id(*body);
            }
        }
    }

    fn check_loop_label(&mut self, label: Option<&yelang_ast::Label>) {
        if let Some(label) = label {
            let found = self
                .loop_labels
                .iter()
                .flatten()
                .any(|l| *l == label.symbol);
            if !found {
                self.error(
                    format!("Label '{}' does not refer to an in-scope loop", label.symbol),
                    Some(label.span),
                );
            }
        } else if self.loop_labels.is_empty() {
            self.error("Break/Continue outside of any loop", None);
        }
    }

    fn validate_crate(&mut self) {
        // The crate root must be a known item.
        self.check_def_id(self.crate_hir.root_module, None);

        // Items are keyed by DefId. Verify each item's def_id matches its slot
        // before walking the tree.
        for (expected_id, opt_item) in self.crate_hir.items.iter_enumerated() {
            if let Some(item) = opt_item {
                if item.def_id != expected_id {
                    self.error(
                        format!(
                            "Item def_id {} does not match its position {}",
                            item.def_id, expected_id
                        ),
                        Some(item.span),
                    );
                }
            }
        }

        walk_crate(self, self.crate_hir);
    }
}

impl<'hir> Visitor<'hir> for Validator<'hir> {
    fn crate_hir(&self) -> Option<&'hir Crate> {
        Some(self.crate_hir)
    }

    fn visit_crate(&mut self) {
        self.validate_crate();
    }

    fn visit_item(&mut self, item: &'hir Item) {
        self.check_def_id(item.def_id, Some(item.span));
        match &item.kind {
            ItemKind::Fn { .. } => self.with_function(|this| walk_item(this, item)),
            _ => walk_item(self, item),
        }
    }

    fn visit_impl_item(&mut self, item: &'hir ImplItem) {
        match &item.kind {
            ImplItemKind::Fn { .. } => {
                self.with_function(|this| crate::visit::visitor::walk_impl_item(this, item));
            }
            _ => crate::visit::visitor::walk_impl_item(self, item),
        }
    }

    fn visit_trait_item(&mut self, item: &'hir TraitItem) {
        match &item.kind {
            TraitItemKind::Fn {
                default: Some(_), ..
            } => self.with_function(|this| crate::visit::visitor::walk_trait_item(this, item)),
            _ => crate::visit::visitor::walk_trait_item(self, item),
        }
    }

    fn visit_body_by_id(&mut self, id: BodyId) {
        if let Some(body) = self.crate_hir.bodies.get(id) {
            self.visit_body(body);
        } else {
            self.error(
                "BodyId is not allocated",
                self.crate_hir.body_spans.get(id).copied(),
            );
        }
    }

    fn visit_expr_by_id(&mut self, id: ExprId) {
        if let Some(expr) = self.crate_hir.exprs.get(id) {
            self.visit_expr(expr);
        } else {
            self.error(
                "ExprId is not allocated",
                self.crate_hir.expr_spans.get(id).copied(),
            );
        }
    }

    fn visit_pat_by_id(&mut self, id: PatId) {
        if let Some(pat) = self.crate_hir.pats.get(id) {
            self.visit_pat(pat);
        } else {
            self.error(
                "PatId is not allocated",
                self.crate_hir.pat_spans.get(id).copied(),
            );
        }
    }

    fn visit_stmt_by_id(&mut self, id: StmtId) {
        if let Some(stmt) = self.crate_hir.stmts.get(id) {
            self.visit_stmt(stmt);
        } else {
            self.error(
                "StmtId is not allocated",
                self.crate_hir.stmt_spans.get(id).copied(),
            );
        }
    }

    fn visit_ty_by_id(&mut self, id: TyId) {
        if let Some(ty) = self.crate_hir.tys.get(id) {
            self.visit_ty(ty);
        } else {
            self.error(
                "TyId is not allocated",
                self.crate_hir.ty_spans.get(id).copied(),
            );
        }
    }

    fn visit_expr(&mut self, expr: &'hir Expr) {
        match expr {
            Expr::Path { res } => self.check_res(res, None),
            Expr::Struct { path, .. } => self.check_res(path, None),
            Expr::MethodCall { trait_def_id, .. } => {
                if let Some(def_id) = trait_def_id {
                    self.check_def_id(*def_id, None);
                }
            }
            Expr::Loop { label, .. } => {
                let label_sym = label.as_ref().map(|l| l.symbol);
                return self.with_loop_label(label_sym, |this| walk_expr(this, expr));
            }
            Expr::Break { label, expr: value } => {
                self.check_loop_label(label.as_ref());
                if let Some(e) = value {
                    self.visit_expr_by_id(*e);
                }
                return;
            }
            Expr::Continue { label } => {
                self.check_loop_label(label.as_ref());
                return;
            }
            Expr::Return { expr: value } => {
                if !self.in_function() {
                    self.error("Return outside of function body", None);
                }
                if let Some(e) = value {
                    self.visit_expr_by_id(*e);
                }
                return;
            }
            Expr::Closure { .. } | Expr::Async { .. } | Expr::Gen { .. } => {
                return self.with_function(|this| walk_expr(this, expr));
            }
            _ => {}
        }
        walk_expr(self, expr);
    }

    fn visit_pat(&mut self, pat: &'hir Pat) {
        match pat {
            Pat::Binding { name, .. } => self.check_binding(*name, None),
            Pat::Rest { name } => {
                if let Some(name) = name {
                    self.check_binding(*name, None);
                }
            }
            _ => {}
        }
        walk_pat(self, pat);
    }

    fn visit_ty(&mut self, ty: &'hir Ty) {
        match ty {
            Ty::Path { res, .. } => self.check_res(res, None),
            Ty::Array { len, .. } => self.check_const(len, Some(len.span)),
            _ => {}
        }
        walk_ty(self, ty);
    }

    fn visit_trait_bound(&mut self, bound: &'hir TraitBound) {
        self.check_res(&bound.path, Some(bound.span));
    }

    fn visit_trait_ref(&mut self, trait_ref: &'hir TraitRef) {
        self.check_res(&trait_ref.path, Some(trait_ref.span));
    }

    fn visit_use_path(&mut self, path: &'hir UsePath) {
        self.check_res(&path.res, Some(path.span));
    }
}

