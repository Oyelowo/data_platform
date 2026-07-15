//! Main lowering entry point and `LoweringContext`.

use yelang_ast::{Item as AstItem, ItemKind as AstItemKind, Program};
use yelang_interner::{Interner, Symbol};
use yelang_lexer::Span;
use yelang_util::{DefId, FxHashMap};

use crate::crate_hir::Crate;
use crate::ids::{BodyId, HirId};
use crate::lowering_err::LoweringError;
use crate::res::ResolvedCrate;

/// Context that drives AST -> HIR lowering.
pub struct LoweringContext<'a> {
    pub interner: &'a Interner,
    pub resolved: &'a ResolvedCrate,
    pub crate_hir: Crate,
    pub next_hir_id: u32,
    pub next_body_id: u32,
    pub current_module: DefId,
    pub current_owner: DefId,
    pub local_map: FxHashMap<Symbol, HirId>,
    pub errors: Vec<LoweringError>,
    /// The `DefId` of the type that `Self` refers to inside the current
    /// `impl` or `trait` block. `None` when not inside such a block.
    pub self_type: Option<DefId>,
}

impl<'a> LoweringContext<'a> {
    pub fn new(interner: &'a Interner, resolved: &'a ResolvedCrate) -> Self {
        let root_module = resolved.module_tree.root.def_id;
        Self {
            interner,
            resolved,
            crate_hir: Crate::new(root_module),
            next_hir_id: 1,
            next_body_id: 1,
            current_module: root_module,
            current_owner: root_module,
            local_map: FxHashMap::new(),
            errors: Vec::new(),
            self_type: None,
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
pub fn lower_crate(program: &Program, resolved: &ResolvedCrate, interner: &Interner) -> Crate {
    let mut ctx = LoweringContext::new(interner, resolved);

    for item in &program.items {
        let _ = crate::lowering_item::lower_item(&mut ctx, item);
    }

    ctx.crate_hir
}

/// Look up the `DefId` for an AST item within the current module.
/// Matches by parent module, name, and kind.
pub(crate) fn lookup_item_def_id(ctx: &LoweringContext, item: &AstItem) -> Option<DefId> {
    use yelang_resolve::DefKind;
    let expected_kind = item_def_kind(&item.kind)?;
    let expected_name = item_name(item)?;
    ctx.resolved
        .definitions
        .iter()
        .find(|(_, def)| {
            def.parent == Some(ctx.current_module)
                && def.name == expected_name
                && def.kind == expected_kind
        })
        .map(|(id, _)| *id)
}

fn item_def_kind(kind: &AstItemKind) -> Option<yelang_resolve::DefKind> {
    use yelang_resolve::DefKind;
    Some(match kind {
        AstItemKind::Fn(_) => DefKind::Fn,
        AstItemKind::Struct(_) => DefKind::Struct,
        AstItemKind::Enum(_) => DefKind::Enum,
        AstItemKind::TypeAlias(_) => DefKind::TypeAlias,
        AstItemKind::Trait(_) => DefKind::Trait,
        AstItemKind::Module(_) => DefKind::Module,
        AstItemKind::Const(_) => DefKind::Const,
        AstItemKind::Static(_) => DefKind::Static,
        AstItemKind::Impl(_) => DefKind::Impl,
        AstItemKind::Use(_) => DefKind::Use,
    })
}

fn item_name(item: &AstItem) -> Option<Symbol> {
    Some(match &item.kind {
        AstItemKind::Fn(f) => f.name.symbol,
        AstItemKind::Struct(s) => s.name.symbol,
        AstItemKind::Enum(e) => e.name.symbol,
        AstItemKind::TypeAlias(t) => t.name.symbol,
        AstItemKind::Trait(t) => t.name.symbol,
        AstItemKind::Module(m) => m.name.symbol,
        AstItemKind::Const(c) => c.name.symbol,
        AstItemKind::Static(s) => s.name.symbol,
        AstItemKind::Impl(_) | AstItemKind::Use(_) => return None,
    })
}
