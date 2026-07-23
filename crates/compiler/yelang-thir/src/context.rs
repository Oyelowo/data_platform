//! Lowering context for HIR → THIR.

use yelang_arena::FxHashMap;
use yelang_hir::ids::{ExprId, PatId};
use yelang_hir::Crate as HirCrate;
use yelang_interner::Interner;
use yelang_resolve::lang_items::LangItems;
use yelang_tycheck::TypeckResults;
use yelang_ty::ty::TyId;
use slotmap::SlotMap;

use crate::body::{ThirBodies, ThirBody};
use crate::expr::ThirExpr;
use crate::ids::{ThirBodyId, ThirExprId, ThirPatId, ThirStmtId};
use crate::pat::ThirPat;
use crate::stmt::ThirStmt;
use crate::ty::ThirTyId;

/// Context used while lowering a single HIR body to THIR.
pub struct LoweringContext<'a> {
    pub hir: &'a HirCrate,
    pub typeck_results: &'a TypeckResults,
    pub lang_items: &'a LangItems,
    pub interner: &'a Interner,
    pub bodies: ThirBodies,
    pub exprs: SlotMap<ThirExprId, ThirExpr>,
    /// Inferred type for each THIR expression, copied from `TypeckResults`.
    pub expr_tys: slotmap::SecondaryMap<ThirExprId, TyId>,
    pub pats: SlotMap<ThirPatId, ThirPat>,
    pub stmts: SlotMap<ThirStmtId, ThirStmt>,
    /// Mapping from HIR pattern ids to THIR pattern ids for the current body.
    pub local_pats: FxHashMap<PatId, ThirPatId>,
    /// Mapping from HIR expression ids to THIR expression ids.
    /// Populated during lowering; used by the plan extraction to convert
    /// HIR ExprId references to THIR ExprRef (ThirExprId).
    pub expr_mapping: FxHashMap<ExprId, ThirExprId>,
}

impl<'a> LoweringContext<'a> {
    pub fn new(
        hir: &'a HirCrate,
        typeck_results: &'a TypeckResults,
        lang_items: &'a LangItems,
        interner: &'a Interner,
    ) -> Self {
        Self {
            hir,
            typeck_results,
            lang_items,
            interner,
            bodies: ThirBodies::default(),
            exprs: SlotMap::with_key(),
            expr_tys: slotmap::SecondaryMap::new(),
            pats: SlotMap::with_key(),
            stmts: SlotMap::with_key(),
            local_pats: FxHashMap::default(),
            expr_mapping: FxHashMap::default(),
        }
    }

    pub fn alloc_expr(&mut self, expr: ThirExpr) -> ThirExprId {
        self.exprs.insert(expr)
    }

    /// Allocate a THIR expression and record its inferred type from the
    /// type-check results for the source HIR expression.
    /// Also records the HIR → THIR expression mapping.
    pub fn alloc_expr_with_ty(
        &mut self,
        expr: ThirExpr,
        source_hir_expr: ExprId,
    ) -> ThirExprId {
        let id = self.exprs.insert(expr);
        if let Some(ty) = self.typeck_results.expr_ty(source_hir_expr) {
            self.expr_tys.insert(id, ty);
        }
        self.expr_mapping.insert(source_hir_expr, id);
        id
    }

    /// Return the inferred type of a THIR expression, if known.
    pub fn thir_expr_ty(&self, expr_id: ThirExprId) -> Option<TyId> {
        self.expr_tys.get(expr_id).copied()
    }

    pub fn alloc_pat(&mut self, pat: ThirPat) -> ThirPatId {
        self.pats.insert(pat)
    }

    pub fn alloc_stmt(&mut self, stmt: ThirStmt) -> ThirStmtId {
        self.stmts.insert(stmt)
    }

    pub fn alloc_body(&mut self, params: Vec<ThirPatId>, value: ThirExprId) -> ThirBodyId {
        self.bodies.alloc(params, value)
    }

    pub fn expr_ty(&self, expr_id: ExprId) -> Option<ThirTyId> {
        self.typeck_results.expr_ty(expr_id).map(ThirTyId)
    }

    pub fn pat_ty(&self, pat_id: PatId) -> Option<ThirTyId> {
        self.typeck_results.pat_ty(pat_id).map(ThirTyId)
    }

    pub fn local_ty(&self, pat_id: PatId) -> Option<ThirTyId> {
        self.typeck_results.local_ty(pat_id).map(ThirTyId)
    }

    /// Look up the THIR body for `id`.
    pub fn body(&self, id: ThirBodyId) -> Option<&ThirBody> {
        self.bodies.bodies.get(id)
    }

    /// Resolve a `Symbol` to its textual form.
    pub fn resolve_symbol(&self, symbol: yelang_interner::Symbol) -> &str {
        self.interner.resolve(&symbol)
    }

    /// Consume the context and return the accumulated [`ThirBodies`],
    /// including the THIR expression arena, `expr_mapping`, and
    /// `query_lowerings` side tables.
    pub fn finish(mut self) -> ThirBodies {
        self.bodies.exprs = self.exprs;
        self.bodies.pats = self.pats;
        self.bodies.expr_mapping = self.expr_mapping;
        self.bodies
    }
}
