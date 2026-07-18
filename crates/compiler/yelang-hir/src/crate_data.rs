//! The HIR crate root.
//!
//! Items are stored out-of-band in dense `IndexVec`s keyed by `DefId`,
//! matching the allocation discipline used by name resolution.  Expressions,
//! patterns, statements, types, and bodies are stored in slotmap arenas with
//! separate per-node IDs.
use yelang_arena::{Arena, ArenaMap, IndexVec};
use yelang_lexer::Span;

use crate::hir::core::{ForeignItem, ForeignItemKind, Impl, ImplItemKind, Trait, TraitItemKind};
use crate::hir::item::ItemKind;
use crate::ids::{BodyId, DefId, ExprId, ForeignItemKindId, ImplItemKindId, ItemKindId, PatId, StmtId, TraitItemKindId, TyId};

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
    pub bodies: Arena<BodyId, Body>,
    /// All expression nodes keyed by `ExprId`.
    pub exprs: Arena<ExprId, Expr>,
    /// All pattern nodes keyed by `PatId`.
    pub pats: Arena<PatId, Pat>,
    /// All statement nodes keyed by `StmtId`.
    pub stmts: Arena<StmtId, Stmt>,
    /// All type nodes keyed by `TyId`.
    pub tys: Arena<TyId, Ty>,
    /// All item-kind payloads keyed by `ItemKindId`.
    pub item_kinds: Arena<ItemKindId, ItemKind>,
    /// All trait-item-kind payloads keyed by `TraitItemKindId`.
    pub trait_item_kinds: Arena<TraitItemKindId, TraitItemKind>,
    /// All impl-item-kind payloads keyed by `ImplItemKindId`.
    pub impl_item_kinds: Arena<ImplItemKindId, ImplItemKind>,
    /// All foreign-item-kind payloads keyed by `ForeignItemKindId`.
    pub foreign_item_kinds: Arena<ForeignItemKindId, ForeignItemKind>,
    /// Secondary map from `ExprId` to the source span of the expression.
    pub expr_spans: ArenaMap<ExprId, Span>,
    /// Secondary map from `PatId` to the source span of the pattern.
    pub pat_spans: ArenaMap<PatId, Span>,
    /// Secondary map from `StmtId` to the source span of the statement.
    pub stmt_spans: ArenaMap<StmtId, Span>,
    /// Secondary map from `TyId` to the source span of the type.
    pub ty_spans: ArenaMap<TyId, Span>,
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
            item_kinds: Arena::new(),
            trait_item_kinds: Arena::new(),
            impl_item_kinds: Arena::new(),
            foreign_item_kinds: Arena::new(),
            expr_spans: ArenaMap::new(),
            pat_spans: ArenaMap::new(),
            stmt_spans: ArenaMap::new(),
            ty_spans: ArenaMap::new(),
            body_spans: ArenaMap::new(),
        }
    }

    /// Allocate an expression node and its span, returning the `ExprId`.
    pub fn alloc_expr(&mut self, expr: Expr, span: Span) -> ExprId {
        let id = self.exprs.insert(expr);
        self.expr_spans.insert(id, span);
        id
    }

    /// Allocate a pattern node and its span, returning the `PatId`.
    pub fn alloc_pat(&mut self, pat: Pat, span: Span) -> PatId {
        let id = self.pats.insert(pat);
        self.pat_spans.insert(id, span);
        id
    }

    /// Allocate a statement node and its span, returning the `StmtId`.
    pub fn alloc_stmt(&mut self, stmt: Stmt, span: Span) -> StmtId {
        let id = self.stmts.insert(stmt);
        self.stmt_spans.insert(id, span);
        id
    }

    /// Allocate a type node and its span, returning the `TyId`.
    pub fn alloc_ty(&mut self, ty: Ty, span: Span) -> TyId {
        let id = self.tys.insert(ty);
        self.ty_spans.insert(id, span);
        id
    }

    /// Allocate a body node and its span, returning the `BodyId`.
    pub fn alloc_body(&mut self, body: Body, span: Span) -> BodyId {
        let id = self.bodies.insert(body);
        self.body_spans.insert(id, span);
        id
    }

    /// Allocate an item-kind payload, returning the `ItemKindId`.
    pub fn alloc_item_kind(&mut self, kind: ItemKind) -> ItemKindId {
        self.item_kinds.insert(kind)
    }

    /// Allocate a trait-item-kind payload, returning the `TraitItemKindId`.
    pub fn alloc_trait_item_kind(&mut self, kind: TraitItemKind) -> TraitItemKindId {
        self.trait_item_kinds.insert(kind)
    }

    /// Allocate an impl-item-kind payload, returning the `ImplItemKindId`.
    pub fn alloc_impl_item_kind(&mut self, kind: ImplItemKind) -> ImplItemKindId {
        self.impl_item_kinds.insert(kind)
    }

    /// Allocate a foreign-item-kind payload, returning the `ForeignItemKindId`.
    pub fn alloc_foreign_item_kind(&mut self, kind: ForeignItemKind) -> ForeignItemKindId {
        self.foreign_item_kinds.insert(kind)
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
    pub fn ty_span(&self, id: TyId) -> Span {
        *self
            .ty_spans
            .get(id)
            .expect("TyId should have an associated span")
    }

    /// Look up the source span of a body.
    pub fn body_span(&self, id: BodyId) -> Span {
        *self
            .body_spans
            .get(id)
            .expect("BodyId should have an associated span")
    }
}

// Re-export `Body` and `Item` so that `crate_hir.rs` can reference them in the
// `Crate` struct above without needing an extra import everywhere.
pub use crate::hir::body::Body;
pub use crate::hir::expr::Expr;
pub use crate::hir::pat::Pat;
pub use crate::hir::ty::Ty;
pub use crate::hir::core::Stmt;
pub use crate::hir::item::Item;
