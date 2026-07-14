//! Main lowering entry point and `LoweringContext`.

use yelang_ast::{Program, Item as AstItem, ItemKind as AstItemKind};
use yelang_interner::{Interner, Symbol};
use yelang_lexer::Span;
use yelang_util::FxHashMap;

use crate::crate_hir::Crate;
use crate::ids::{DefId, HirId, BodyId};
use crate::lowering_err::LoweringError;
use crate::res::ResolvedCrate;

/// Context that drives AST -> HIR lowering.
pub struct LoweringContext<'a> {
    pub interner: &'a Interner,
    pub resolved: &'a ResolvedCrate,
    pub crate_hir: Crate,
    pub next_hir_id: u32,
    pub next_body_id: u32,
    pub current_owner: DefId,
    pub local_map: FxHashMap<Symbol, HirId>,
    pub errors: Vec<LoweringError>,
}

impl<'a> LoweringContext<'a> {
    pub fn new(interner: &'a Interner, resolved: &'a ResolvedCrate, root_module: DefId) -> Self {
        Self {
            interner,
            resolved,
            crate_hir: Crate::new(root_module),
            next_hir_id: 1,
            next_body_id: 1,
            current_owner: root_module,
            local_map: FxHashMap::new(),
            errors: Vec::new(),
        }
    }

    /// Allocate a fresh `HirId`.
    pub fn next_hir_id(&mut self) -> HirId {
        let id = HirId::new(self.next_hir_id);
        self.next_hir_id += 1;
        id
    }

    /// Allocate a fresh `BodyId`.
    pub fn next_body_id(&mut self) -> BodyId {
        let id = BodyId::new(self.next_body_id);
        self.next_body_id += 1;
        id
    }

    /// Allocate a fresh `DefId`.
    pub fn next_def_id(&mut self) -> DefId {
        let id = DefId::new(self.next_body_id);
        self.next_body_id += 1;
        id
    }

    /// Record a lowering error.
    pub fn error(&mut self, err: LoweringError) {
        self.errors.push(err);
    }

    /// Push a local variable into scope.
    pub fn push_local(&mut self, name: Symbol, hir_id: HirId) {
        self.local_map.insert(name, hir_id);
    }

    /// Pop a local variable from scope.
    pub fn pop_local(&mut self, name: Symbol) {
        self.local_map.remove(&name);
    }

    /// Look up a local variable.
    pub fn local(&self, name: Symbol) -> Option<HirId> {
        self.local_map.get(&name).copied()
    }
}

/// Lower an entire AST `Program` into a HIR `Crate`.
pub fn lower_crate(
    program: &Program,
    resolved: &ResolvedCrate,
    interner: &Interner,
) -> Crate {
    let mut ctx = LoweringContext::new(interner, resolved, resolved.root_module);

    for item in &program.items {
        let _ = crate::lowering_item::lower_item(&mut ctx, item);
    }

    ctx.crate_hir
}
