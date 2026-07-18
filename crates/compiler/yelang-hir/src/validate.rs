//! HIR validation pass.
//!
//! Checks structural invariants of the HIR without panicking on malformed input.

use std::collections::HashSet;

use yelang_interner::Symbol;
use yelang_lexer::Span;

use crate::crate_hir::Crate;
use crate::hir::{
    Arm, Block, EnumDef, Expr, FieldDef, FnSig, ForeignItem, ForeignItemKind, GenericParam,
    Generics, Impl, ImplItem, ImplItemKind, Item, ItemKind, Stmt, StructField, Trait, TraitBound,
    TraitItem, TraitItemKind, TraitRef, UseKind, UsePath, VariantData, VariantDef, WhereClause,
    WherePredicate,
};
use crate::hir_body::{Body, Param};
use crate::hir_pat::Pat;
use crate::hir_ty::{Const, ConstKind, GenericArg, Ty};
use crate::ids::{BodyId, DefId, ExprId, PatId, StmtId, TyId};
use crate::res::Res;

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
    /// Binding names introduced in the current function body scope.
    bindings: HashSet<Symbol>,
}

impl<'hir> Validator<'hir> {
    fn new(crate_hir: &'hir Crate) -> Self {
        Self {
            crate_hir,
            errors: Vec::new(),
            function_depth: 0,
            loop_labels: Vec::new(),
            bindings: HashSet::new(),
        }
    }

    fn error(&mut self, message: impl Into<String>, span: Option<Span>) {
        self.errors.push(ValidationError {
            message: message.into(),
            span,
        });
    }

    // -------------------------------------------------------------------------
    // ID existence checks
    // -------------------------------------------------------------------------

    fn check_def_id(&mut self, def_id: DefId, span: Option<Span>) {
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
                self.check_pat_id(*pat_id, span);
            }
            Res::PrimTy { .. } | Res::Err => {}
        }
    }

    fn check_expr_id(&mut self, id: ExprId, span: Option<Span>) -> Option<&'hir Expr> {
        if let Some(expr) = self.crate_hir.exprs.get(id) {
            Some(expr)
        } else {
            self.error(format!("ExprId is not allocated"), span);
            None
        }
    }

    fn check_pat_id(&mut self, id: PatId, span: Option<Span>) -> Option<&'hir Pat> {
        if let Some(pat) = self.crate_hir.pats.get(id) {
            Some(pat)
        } else {
            self.error(format!("PatId is not allocated"), span);
            None
        }
    }

    fn check_stmt_id(&mut self, id: StmtId, span: Option<Span>) -> Option<&'hir Stmt> {
        if let Some(stmt) = self.crate_hir.stmts.get(id) {
            Some(stmt)
        } else {
            self.error(format!("StmtId is not allocated"), span);
            None
        }
    }

    fn check_ty_id(&mut self, id: TyId, span: Option<Span>) -> Option<&'hir Ty> {
        if let Some(ty) = self.crate_hir.tys.get(id) {
            Some(ty)
        } else {
            self.error(format!("TyId is not allocated"), span);
            None
        }
    }

    fn check_body_id(&mut self, id: BodyId, span: Option<Span>) -> Option<&'hir Body> {
        if let Some(body) = self.crate_hir.bodies.get(id) {
            Some(body)
        } else {
            self.error(format!("BodyId is not allocated"), span);
            None
        }
    }

    // -------------------------------------------------------------------------
    // Recursive validators
    // -------------------------------------------------------------------------

    fn validate_crate(&mut self) {
        // The crate root must be a known item.
        self.check_def_id(self.crate_hir.root_module, None);

        // Items are keyed by DefId. Verify each item's def_id matches its slot
        // and then validate the item itself.
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
                self.validate_item(item);
            }
        }

        for impl_ in &self.crate_hir.impls {
            self.validate_impl(impl_);
        }

        for opt_trait in self.crate_hir.traits.iter() {
            if let Some(trait_) = opt_trait {
                self.validate_trait(trait_);
            }
        }

        for opt_foreign in self.crate_hir.foreign_items.iter() {
            if let Some(foreign) = opt_foreign {
                self.validate_foreign_item(foreign);
            }
        }
    }

    fn validate_item(&mut self, item: &'hir Item) {
        match &item.kind {
            ItemKind::Fn { sig, body, generics } => {
                self.validate_fn_sig(sig);
                self.validate_generics(generics);
                self.validate_body(*body, true);
            }
            ItemKind::Struct { data, generics } => {
                self.validate_variant_data(data);
                self.validate_generics(generics);
            }
            ItemKind::Enum { def, generics } => {
                self.validate_enum_def(def);
                self.validate_generics(generics);
            }
            ItemKind::Trait {
                items,
                generics,
                super_traits,
            } => {
                self.validate_trait_items(items);
                self.validate_generics(generics);
                for super_trait in super_traits {
                    self.validate_trait_ref(super_trait);
                }
            }
            ItemKind::Impl {
                items,
                generics,
                self_ty,
                of_trait,
                polarity: _,
            } => {
                self.validate_generics(generics);
                self.validate_ty_id(*self_ty, Some(item.span));
                if let Some(trait_ref) = of_trait {
                    self.validate_trait_ref(trait_ref);
                }
                for impl_item in items {
                    self.validate_impl_item(impl_item);
                }
            }
            ItemKind::TyAlias { ty, generics } => {
                self.validate_ty_id(*ty, Some(item.span));
                self.validate_generics(generics);
            }
            ItemKind::Const { ty, body } => {
                self.validate_ty_id(*ty, Some(item.span));
                self.validate_body(*body, false);
            }
            ItemKind::Static { ty, body, .. } => {
                self.validate_ty_id(*ty, Some(item.span));
                self.validate_body(*body, false);
            }
            ItemKind::Mod { items } => {
                for &def_id in items {
                    self.check_def_id(def_id, Some(item.span));
                }
            }
            ItemKind::Use { path, kind } => {
                self.validate_use_path(path);
                self.validate_use_kind(kind);
            }
        }
    }

    fn validate_body(&mut self, body_id: BodyId, is_function: bool) {
        let Some(body) = self.check_body_id(body_id, self.crate_hir.body_spans.get(body_id).copied()) else {
            return;
        };

        let saved_bindings = if is_function {
            self.function_depth += 1;
            Some(std::mem::take(&mut self.bindings))
        } else {
            None
        };

        for param in &body.params {
            self.validate_param(param);
        }
        self.validate_expr_id(body.value, self.crate_hir.body_spans.get(body_id).copied());

        if is_function {
            self.function_depth -= 1;
            self.bindings = saved_bindings.unwrap();
        }
    }

    fn validate_param(&mut self, param: &'hir Param) {
        self.validate_pat_id(param.pat, Some(param.span));
        self.validate_ty_id(param.ty, Some(param.span));
    }

    fn validate_expr_id(&mut self, id: ExprId, fallback_span: Option<Span>) {
        let span = self.crate_hir.expr_spans.get(id).copied().or(fallback_span);
        if let Some(expr) = self.check_expr_id(id, span) {
            self.validate_expr(expr);
        }
    }

    fn validate_expr(&mut self, expr: &'hir Expr) {
        match expr {
            Expr::Lit { .. } => {}
            Expr::Path { res } => {
                self.check_res(res, None);
            }
            Expr::Binary { left, right, .. } => {
                self.validate_expr_id(*left, None);
                self.validate_expr_id(*right, None);
            }
            Expr::Unary { expr: inner, .. } => {
                self.validate_expr_id(*inner, None);
            }
            Expr::Call { func, args } => {
                self.validate_expr_id(*func, None);
                for arg in args {
                    self.validate_expr_id(*arg, None);
                }
            }
            Expr::MethodCall {
                receiver,
                args,
                trait_def_id,
                ..
            } => {
                self.validate_expr_id(*receiver, None);
                for arg in args {
                    self.validate_expr_id(*arg, None);
                }
                if let Some(def_id) = trait_def_id {
                    self.check_def_id(*def_id, None);
                }
            }
            Expr::Field { expr: inner, .. } => {
                self.validate_expr_id(*inner, None);
            }
            Expr::Index { expr: inner, index } => {
                self.validate_expr_id(*inner, None);
                self.validate_expr_id(*index, None);
            }
            Expr::Assign { left, right } => {
                self.validate_expr_id(*left, None);
                self.validate_expr_id(*right, None);
            }
            Expr::Block { block } => {
                self.validate_block(block);
            }
            Expr::Loop { block, label } => {
                self.loop_labels.push(label.as_ref().map(|l| l.symbol));
                self.validate_block(block);
                self.loop_labels.pop();
            }
            Expr::Break { label, expr } => {
                self.check_loop_label(label.as_ref());
                if let Some(e) = expr {
                    self.validate_expr_id(*e, None);
                }
            }
            Expr::Continue { label } => {
                self.check_loop_label(label.as_ref());
            }
            Expr::Return { expr } => {
                if self.function_depth == 0 {
                    self.error("Return outside of function body".to_string(), None);
                }
                if let Some(e) = expr {
                    self.validate_expr_id(*e, None);
                }
            }
            Expr::Match { expr, arms } => {
                self.validate_expr_id(*expr, None);
                for arm in arms {
                    self.validate_arm(arm);
                }
            }
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.validate_expr_id(*cond, None);
                self.validate_expr_id(*then_branch, None);
                if let Some(e) = else_branch {
                    self.validate_expr_id(*e, None);
                }
            }
            Expr::Closure { params, body, .. } => {
                for param in params {
                    self.validate_param(param);
                }
                self.validate_body(*body, false);
            }
            Expr::Struct { path, fields, rest } => {
                self.check_res(path, None);
                for field in fields {
                    self.validate_expr_id(field.expr, Some(field.span));
                }
                if let Some(e) = rest {
                    self.validate_expr_id(*e, None);
                }
            }
            Expr::Tuple { exprs } | Expr::Array { exprs } => {
                for e in exprs {
                    self.validate_expr_id(*e, None);
                }
            }
            Expr::Cast { expr: inner, ty } => {
                self.validate_expr_id(*inner, None);
                self.validate_ty_id(*ty, None);
            }
            Expr::Let { pat, expr: inner } => {
                self.validate_pat_id(*pat, None);
                self.validate_expr_id(*inner, None);
            }
            Expr::AssignOp { left, right, .. } => {
                self.validate_expr_id(*left, None);
                self.validate_expr_id(*right, None);
            }
            Expr::DestructureAssign { pat, value } => {
                self.validate_pat_id(*pat, None);
                self.validate_expr_id(*value, None);
            }
            Expr::Range { start, end, .. } => {
                if let Some(e) = start {
                    self.validate_expr_id(*e, None);
                }
                if let Some(e) = end {
                    self.validate_expr_id(*e, None);
                }
            }
            Expr::Object { fields } => {
                for field in fields {
                    self.validate_expr_id(field.expr, Some(field.span));
                }
            }
            Expr::IsType { expr: inner, ty } | Expr::TypeAscription { expr: inner, ty } => {
                self.validate_expr_id(*inner, None);
                self.validate_ty_id(*ty, None);
            }
            Expr::Try { expr: inner } | Expr::Await { expr: inner } => {
                self.validate_expr_id(*inner, None);
            }
            Expr::Async { body } | Expr::Gen { body, .. } => {
                self.validate_body(*body, false);
            }
            Expr::DocumentAccess { base, projection } => {
                self.validate_expr_id(*base, None);
                for proj in projection {
                    match proj {
                        crate::hir_expr::DocumentProjection::Field { value, .. } => {
                            if let Some(e) = value {
                                self.validate_expr_id(*e, None);
                            }
                        }
                        crate::hir_expr::DocumentProjection::Spread(e) => {
                            self.validate_expr_id(*e, None);
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
                self.validate_expr_id(*element, None);
                for (pat, source) in variables {
                    self.validate_pat_id(*pat, None);
                    self.validate_expr_id(*source, None);
                }
                if let Some(cond) = condition {
                    self.validate_expr_id(*cond, None);
                }
            }
            Expr::Err => {}
        }
    }

    fn validate_pat_id(&mut self, id: PatId, fallback_span: Option<Span>) {
        let span = self.crate_hir.pat_spans.get(id).copied().or(fallback_span);
        if let Some(pat) = self.check_pat_id(id, span) {
            self.validate_pat(pat);
        }
    }

    fn validate_pat(&mut self, pat: &'hir Pat) {
        match pat {
            Pat::Wild => {}
            Pat::Binding { name, subpat, .. } => {
                if !self.bindings.insert(*name) {
                    self.error(
                        format!("Duplicate binding name '{}' in function body", name),
                        None,
                    );
                }
                if let Some(p) = subpat {
                    self.validate_pat_id(*p, None);
                }
            }
            Pat::Struct { res, fields, .. } => {
                self.check_res(res, None);
                for field in fields {
                    self.validate_pat_id(field.pat, Some(field.span));
                }
            }
            Pat::Tuple { pats } => {
                for p in pats {
                    self.validate_pat_id(*p, None);
                }
            }
            Pat::TupleStruct { res, pats } => {
                self.check_res(res, None);
                for p in pats {
                    self.validate_pat_id(*p, None);
                }
            }
            Pat::Path { res } => {
                self.check_res(res, None);
            }
            Pat::Lit { .. } => {}
            Pat::Range { start, end, .. } => {
                if let Some(s) = start {
                    self.validate_pat_id(*s, None);
                }
                if let Some(e) = end {
                    self.validate_pat_id(*e, None);
                }
            }
            Pat::Or { pats } => {
                for p in pats {
                    self.validate_pat_id(*p, None);
                }
            }
            Pat::Slice {
                prefix,
                middle,
                suffix,
            } => {
                for p in prefix {
                    self.validate_pat_id(*p, None);
                }
                if let Some(m) = middle {
                    self.validate_pat_id(*m, None);
                }
                for p in suffix {
                    self.validate_pat_id(*p, None);
                }
            }
            Pat::Ref { pat, .. } => {
                self.validate_pat_id(*pat, None);
            }
            Pat::Rest { name } => {
                if let Some(name) = name {
                    if !self.bindings.insert(*name) {
                        self.error(
                            format!("Duplicate binding name '{}' in function body", name),
                            None,
                        );
                    }
                }
            }
            Pat::Err => {}
        }
    }

    fn validate_stmt_id(&mut self, id: StmtId, fallback_span: Option<Span>) {
        let span = self.crate_hir.stmt_spans.get(id).copied().or(fallback_span);
        if let Some(stmt) = self.check_stmt_id(id, span) {
            self.validate_stmt(stmt);
        }
    }

    fn validate_stmt(&mut self, stmt: &'hir Stmt) {
        match stmt {
            Stmt::Expr { expr } => {
                self.validate_expr_id(*expr, None);
            }
            Stmt::Let { pat, ty, init } => {
                self.validate_pat_id(*pat, None);
                if let Some(t) = ty {
                    self.validate_ty_id(*t, None);
                }
                if let Some(e) = init {
                    self.validate_expr_id(*e, None);
                }
            }
            Stmt::Item { item } => {
                self.validate_item(item);
            }
        }
    }

    fn validate_ty_id(&mut self, id: TyId, fallback_span: Option<Span>) {
        let span = self.crate_hir.ty_spans.get(id).copied().or(fallback_span);
        if let Some(ty) = self.check_ty_id(id, span) {
            self.validate_ty(ty);
        }
    }

    fn validate_ty(&mut self, ty: &'hir Ty) {
        match ty {
            Ty::Path { res, args } => {
                self.check_res(res, None);
                for arg in args {
                    self.validate_generic_arg(arg);
                }
            }
            Ty::Tuple { tys } => {
                for t in tys {
                    self.validate_ty_id(*t, None);
                }
            }
            Ty::Array { ty, len } => {
                self.validate_ty_id(*ty, None);
                self.check_const(len, Some(len.span));
            }
            Ty::Slice { ty } => {
                self.validate_ty_id(*ty, None);
            }
            Ty::FnPtr { sig } => {
                self.validate_fn_sig(sig);
            }
            Ty::AnonStruct { fields } => {
                for field in fields {
                    self.validate_ty_id(field.ty, None);
                }
            }
            Ty::TypeLit { .. } => {}
            Ty::Utility { args, .. } => {
                for arg in args {
                    self.validate_ty_id(*arg, None);
                }
            }
            Ty::TypeOf { expr } => {
                self.validate_expr_id(*expr, None);
            }
            Ty::Ref { ty, .. } | Ty::RawPtr { ty, .. } => {
                self.validate_ty_id(*ty, None);
            }
            Ty::ForAll { params, ty } => {
                for param in params {
                    self.validate_generic_param(param);
                }
                self.validate_ty_id(*ty, None);
            }
            Ty::Union { tys } => {
                for t in tys {
                    self.validate_ty_id(*t, None);
                }
            }
            Ty::ImplTrait { path } => {
                self.check_res(path, None);
            }
            Ty::DynTrait { path } => {
                self.check_res(path, None);
            }
            Ty::Never | Ty::Infer | Ty::Missing | Ty::Err => {}
        }
    }

    fn validate_generic_arg(&mut self, arg: &'hir GenericArg) {
        match arg {
            GenericArg::Type(ty) => self.validate_ty_id(*ty, None),
            GenericArg::Const(konst) => self.check_const(konst, Some(konst.span)),
            GenericArg::AssocBinding { ty, .. } => self.validate_ty_id(*ty, None),
        }
    }

    fn validate_block(&mut self, block: &'hir Block) {
        for stmt in &block.stmts {
            self.validate_stmt_id(*stmt, Some(block.span));
        }
        if let Some(expr) = &block.expr {
            self.validate_expr_id(*expr, Some(block.span));
        }
    }

    fn validate_arm(&mut self, arm: &'hir Arm) {
        self.validate_pat_id(arm.pat, Some(arm.span));
        if let Some(guard) = &arm.guard {
            self.validate_expr_id(*guard, Some(arm.span));
        }
        self.validate_expr_id(arm.body, Some(arm.span));
    }

    fn validate_fn_sig(&mut self, sig: &'hir FnSig) {
        for ty in &sig.inputs {
            self.validate_ty_id(*ty, None);
        }
        self.validate_ty_id(sig.output, None);
    }

    fn validate_variant_data(&mut self, data: &'hir VariantData) {
        match data {
            VariantData::Struct { fields } => {
                for field in fields {
                    self.validate_field_def(field);
                }
            }
            VariantData::Tuple { fields } => {
                for field in fields {
                    self.validate_struct_field(field);
                }
            }
            VariantData::Unit => {}
        }
    }

    fn validate_field_def(&mut self, field: &'hir FieldDef) {
        self.validate_ty_id(field.ty, Some(field.span));
    }

    fn validate_struct_field(&mut self, field: &'hir StructField) {
        self.validate_ty_id(field.ty, Some(field.span));
    }

    fn validate_enum_def(&mut self, def: &'hir EnumDef) {
        for variant in &def.variants {
            self.validate_variant_def(variant);
        }
    }

    fn validate_variant_def(&mut self, variant: &'hir VariantDef) {
        self.validate_variant_data(&variant.data);
        if let Some(discriminant) = &variant.discriminant {
            self.check_const(discriminant, Some(variant.span));
        }
    }

    fn validate_impl(&mut self, impl_: &'hir Impl) {
        self.validate_generics(&impl_.generics);
        self.validate_ty_id(impl_.self_ty, Some(impl_.span));
        if let Some(trait_ref) = &impl_.of_trait {
            self.validate_trait_ref(trait_ref);
        }
        for item in &impl_.items {
            self.validate_impl_item(item);
        }
    }

    fn validate_impl_item(&mut self, item: &'hir ImplItem) {
        match &item.kind {
            ImplItemKind::Fn { sig, body } => {
                self.validate_fn_sig(sig);
                self.validate_body(*body, true);
            }
            ImplItemKind::Const { ty, body } => {
                self.validate_ty_id(*ty, Some(item.span));
                self.validate_body(*body, false);
            }
            ImplItemKind::Type { ty } => {
                self.validate_ty_id(*ty, Some(item.span));
            }
        }
    }

    fn validate_trait(&mut self, trait_: &'hir Trait) {
        self.validate_generics(&trait_.generics);
        for super_trait in &trait_.super_traits {
            self.validate_trait_ref(super_trait);
        }
        for item in &trait_.items {
            self.validate_trait_item(item);
        }
    }

    fn validate_trait_items(&mut self, items: &'hir [TraitItem]) {
        for item in items {
            self.validate_trait_item(item);
        }
    }

    fn validate_trait_item(&mut self, item: &'hir TraitItem) {
        match &item.kind {
            TraitItemKind::Fn { sig, default } => {
                self.validate_fn_sig(sig);
                if let Some(body) = default {
                    self.validate_body(*body, true);
                }
            }
            TraitItemKind::Const { ty, body } => {
                self.validate_ty_id(*ty, Some(item.span));
                if let Some(body) = body {
                    self.validate_body(*body, false);
                }
            }
            TraitItemKind::Type { bounds, default } => {
                for bound in bounds {
                    self.validate_trait_bound(bound);
                }
                if let Some(ty) = default {
                    self.validate_ty_id(*ty, Some(item.span));
                }
            }
        }
    }

    fn validate_foreign_item(&mut self, foreign: &'hir ForeignItem) {
        match &foreign.kind {
            ForeignItemKind::Fn { sig } => {
                self.validate_fn_sig(sig);
            }
            ForeignItemKind::Static { ty, .. } => {
                self.validate_ty_id(*ty, Some(foreign.span));
            }
            ForeignItemKind::Type => {}
        }
    }

    fn validate_generics(&mut self, generics: &'hir Generics) {
        for param in &generics.params {
            self.validate_generic_param(param);
        }
        if let Some(where_clause) = &generics.where_clause {
            self.validate_where_clause(where_clause);
        }
    }

    fn validate_generic_param(&mut self, param: &'hir GenericParam) {
        match param {
            GenericParam::Type {
                bounds,
                default,
                span,
                ..
            } => {
                for bound in bounds {
                    self.validate_trait_bound(bound);
                }
                if let Some(ty) = default {
                    self.validate_ty_id(*ty, Some(*span));
                }
            }
            GenericParam::Const {
                ty,
                default,
                span,
                ..
            } => {
                self.validate_ty_id(*ty, Some(*span));
                if let Some(expr) = default {
                    self.validate_expr_id(*expr, Some(*span));
                }
            }
        }
    }

    fn validate_trait_bound(&mut self, bound: &'hir TraitBound) {
        self.check_res(&bound.path, Some(bound.span));
    }

    fn validate_trait_ref(&mut self, trait_ref: &'hir TraitRef) {
        self.check_res(&trait_ref.path, Some(trait_ref.span));
    }

    fn validate_where_clause(&mut self, clause: &'hir WhereClause) {
        for pred in &clause.predicates {
            match pred {
                WherePredicate::TraitBound { ty, bounds } => {
                    self.validate_ty_id(*ty, Some(clause.span));
                    for bound in bounds {
                        self.validate_trait_bound(bound);
                    }
                }
                WherePredicate::TypeEq { lhs, rhs } => {
                    self.validate_ty_id(*lhs, Some(clause.span));
                    self.validate_ty_id(*rhs, Some(clause.span));
                }
            }
        }
    }

    fn validate_use_path(&mut self, path: &'hir UsePath) {
        self.check_res(&path.res, Some(path.span));
    }

    fn validate_use_kind(&mut self, kind: &'hir UseKind) {
        match kind {
            UseKind::Single | UseKind::Glob => {}
            UseKind::Nested { items } => {
                for item in items {
                    self.validate_use_path(item);
                }
            }
        }
    }

    // -------------------------------------------------------------------------
    // Constant expressions
    // -------------------------------------------------------------------------

    fn check_const(&mut self, konst: &Const, span: Option<Span>) {
        match &konst.kind {
            ConstKind::Lit { .. } => {}
            ConstKind::Expr { body } => {
                self.check_body_id(*body, span);
            }
            ConstKind::Err => {
                self.error("Constant expression is erroneous".to_string(), span);
            }
        }
    }

    // -------------------------------------------------------------------------
    // Control flow
    // -------------------------------------------------------------------------

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
            self.error("Break/Continue outside of any loop".to_string(), None);
        }
    }
}
