//! The HIR crate root.
//!
//! Items are stored out-of-band in dense `IndexVec`s keyed by `DefId`,
//! matching the allocation discipline used by name resolution.  Expressions,
//! patterns, statements, types, and bodies are stored in slotmap arenas with
//! separate per-node IDs.
use yelang_arena::{Arena, ArenaMap, IndexVec};
use yelang_lexer::Span;

use crate::hir::core::{ForeignItem, Impl};
use crate::ids::{BodyId, DefId, ExprId, PatId, StmtId, SyntaxTyId};

/// The root of the HIR for a single compilation unit.
#[derive(Debug, Clone)]
pub struct Crate {
    pub root_module: DefId,
    /// All items keyed by `DefId`.
    pub items: IndexVec<DefId, Option<Item>>,
    /// Trait definitions keyed by the trait's `DefId`.
    pub traits: IndexVec<DefId, Option<Trait>>,
    /// Impl blocks.
    pub impls: Vec<Impl>,
    /// Foreign items from `extern` blocks keyed by their `DefId`.
    pub foreign_items: IndexVec<DefId, Option<ForeignItem>>,
    /// All bodies keyed by `BodyId`.
    pub bodies: Arena<BodyId, Option<Body>>,
    /// All expression nodes keyed by `ExprId`.
    pub exprs: Arena<ExprId, Option<Expr>>,
    /// All pattern nodes keyed by `PatId`.
    pub pats: Arena<PatId, Option<Pat>>,
    /// All statement nodes keyed by `StmtId`.
    pub stmts: Arena<StmtId, Option<Stmt>>,
    /// All type syntax nodes keyed by `SyntaxTyId`.
    pub tys: Arena<SyntaxTyId, Option<Ty>>,
    /// Secondary map from `ExprId` to the source span of the expression.
    pub expr_spans: ArenaMap<ExprId, Span>,
    /// Secondary map from `PatId` to the source span of the pattern.
    pub pat_spans: ArenaMap<PatId, Span>,
    /// Secondary map from `StmtId` to the source span of the statement.
    pub stmt_spans: ArenaMap<StmtId, Span>,
    /// Secondary map from `SyntaxTyId` to the source span of the type.
    pub ty_spans: ArenaMap<SyntaxTyId, Span>,
    /// Secondary map from `BodyId` to the source span of the body.
    pub body_spans: ArenaMap<BodyId, Span>,
}

impl Crate {
    pub fn new(root_module: DefId) -> Self {
        Self {
            root_module,
            items: IndexVec::new(),
            traits: IndexVec::new(),
            impls: Vec::new(),
            foreign_items: IndexVec::new(),
            bodies: Arena::new(),
            exprs: Arena::new(),
            pats: Arena::new(),
            stmts: Arena::new(),
            tys: Arena::new(),
            expr_spans: ArenaMap::new(),
            pat_spans: ArenaMap::new(),
            stmt_spans: ArenaMap::new(),
            ty_spans: ArenaMap::new(),
            body_spans: ArenaMap::new(),
        }
    }

    /// Allocate an expression node and its span, returning the `ExprId`.
    pub fn alloc_expr(&mut self, expr: Expr, span: Span) -> ExprId {
        let id = self.exprs.insert(Some(expr));
        self.expr_spans.insert(id, span);
        id
    }

    /// Allocate a pattern node and its span, returning the `PatId`.
    pub fn alloc_pat(&mut self, pat: Pat, span: Span) -> PatId {
        let id = self.pats.insert(Some(pat));
        self.pat_spans.insert(id, span);
        id
    }

    /// Allocate a statement node and its span, returning the `StmtId`.
    pub fn alloc_stmt(&mut self, stmt: Stmt, span: Span) -> StmtId {
        let id = self.stmts.insert(Some(stmt));
        self.stmt_spans.insert(id, span);
        id
    }

    /// Allocate a type syntax node and its span, returning the `SyntaxTyId`.
    pub fn alloc_ty(&mut self, ty: Ty, span: Span) -> SyntaxTyId {
        let id = self.tys.insert(Some(ty));
        self.ty_spans.insert(id, span);
        id
    }

    /// Allocate a body node and its span, returning the `BodyId`.
    pub fn alloc_body(&mut self, body: Body, span: Span) -> BodyId {
        let id = self.bodies.insert(Some(body));
        self.body_spans.insert(id, span);
        id
    }

    /// Look up an expression node by `ExprId`.
    pub fn expr(&self, id: ExprId) -> Option<&Expr> {
        self.exprs.get(id).and_then(|o| o.as_ref())
    }

    /// Look up a mutable expression node by `ExprId`.
    pub fn expr_mut(&mut self, id: ExprId) -> Option<&mut Expr> {
        self.exprs.get_mut(id).and_then(|o| o.as_mut())
    }

    /// Look up a pattern node by `PatId`.
    pub fn pat(&self, id: PatId) -> Option<&Pat> {
        self.pats.get(id).and_then(|o| o.as_ref())
    }

    /// Look up a mutable pattern node by `PatId`.
    pub fn pat_mut(&mut self, id: PatId) -> Option<&mut Pat> {
        self.pats.get_mut(id).and_then(|o| o.as_mut())
    }

    /// Look up a statement node by `StmtId`.
    pub fn stmt(&self, id: StmtId) -> Option<&Stmt> {
        self.stmts.get(id).and_then(|o| o.as_ref())
    }

    /// Look up a mutable statement node by `StmtId`.
    pub fn stmt_mut(&mut self, id: StmtId) -> Option<&mut Stmt> {
        self.stmts.get_mut(id).and_then(|o| o.as_mut())
    }

    /// Look up a type syntax node by `SyntaxTyId`.
    pub fn ty(&self, id: SyntaxTyId) -> Option<&Ty> {
        self.tys.get(id).and_then(|o| o.as_ref())
    }

    /// Look up a mutable type syntax node by `SyntaxTyId`.
    pub fn ty_mut(&mut self, id: SyntaxTyId) -> Option<&mut Ty> {
        self.tys.get_mut(id).and_then(|o| o.as_mut())
    }

    /// Look up a body node by `BodyId`.
    pub fn body(&self, id: BodyId) -> Option<&Body> {
        self.bodies.get(id).and_then(|o| o.as_ref())
    }

    /// Look up a mutable body node by `BodyId`.
    pub fn body_mut(&mut self, id: BodyId) -> Option<&mut Body> {
        self.bodies.get_mut(id).and_then(|o| o.as_mut())
    }

    /// Look up the source span of an expression.
    pub fn expr_span(&self, id: ExprId) -> Span {
        *self
            .expr_spans
            .get(id)
            .expect("ExprId should have an associated span")
    }

    /// Look up the source span of a pattern.
    pub fn pat_span(&self, id: PatId) -> Span {
        *self
            .pat_spans
            .get(id)
            .expect("PatId should have an associated span")
    }

    /// Look up the source span of a statement.
    pub fn stmt_span(&self, id: StmtId) -> Span {
        *self
            .stmt_spans
            .get(id)
            .expect("StmtId should have an associated span")
    }

    /// Look up the source span of a type.
    pub fn ty_span(&self, id: SyntaxTyId) -> Span {
        *self
            .ty_spans
            .get(id)
            .expect("SyntaxTyId should have an associated span")
    }

    /// Look up the source span of a body.
    pub fn body_span(&self, id: BodyId) -> Span {
        *self
            .body_spans
            .get(id)
            .expect("BodyId should have an associated span")
    }
}

// Re-export `Body` and `Item` so that `crate_data.rs` can reference them in the
// `Crate` struct above without needing an extra import everywhere.
pub use crate::hir::body::Body;
pub use crate::hir::expr::Expr;
pub use crate::hir::pat::Pat;
pub use crate::hir::ty::Ty;
pub use crate::hir::core::Stmt;
pub use crate::hir::core::Trait;
pub use crate::hir::item::Item;
