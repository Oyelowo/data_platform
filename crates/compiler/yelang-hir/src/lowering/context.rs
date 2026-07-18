//! Main lowering entry point and `LoweringContext`.

use yelang_arena::DefId;
use yelang_ast::{Item as AstItem, ItemKind as AstItemKind, Program};
use yelang_interner::{Interner, Symbol};

use crate::crate_data::Crate;
use crate::ids::PatId;
use crate::lowering::err::LoweringError;
use crate::res::ResolvedCrate;

/// Context that drives AST -> HIR lowering.
pub struct LoweringContext<'a> {
    pub interner: &'a Interner,
    pub resolved: &'a ResolvedCrate,
    pub crate_hir: Crate,
    /// Number of synthetic `DefId`s allocated beyond the IDs produced during
    /// name resolution. The first synthesized ID is `definitions.len() + 1`.
    pub synthetic_def_count: u32,
    pub current_module: DefId,
    pub current_owner: DefId,
    /// Local variable bindings, scoped by block/function body. Each scope is a
    /// map from name to the `PatId` that introduced the binding. Searching from
    /// the top of the stack gives the innermost binding, matching Rust's
    /// lexical scoping and preventing bindings from leaking across functions.
    pub local_scopes: Vec<yelang_arena::FxHashMap<Symbol, PatId>>,
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
            synthetic_def_count: 0,
            current_module: root_module,
            current_owner: root_module,
            local_scopes: vec![yelang_arena::FxHashMap::new()],
            errors: Vec::new(),
            self_type: None,
        }
    }

    /// Allocate a fresh synthetic `DefId` for compiler-generated items (e.g.
    /// derived impls). Synthetic IDs are derived from the definition arena so
    /// they never collide with user-defined or prelude definitions.
    pub fn next_synthetic_def_id(&mut self) -> DefId {
        let raw = self.resolved.definitions.len() as u32 + self.synthetic_def_count + 1;
        self.synthetic_def_count += 1;
        DefId::new(raw)
    }

    /// Record a lowering error.
    pub fn error(&mut self, err: LoweringError) {
        self.errors.push(err);
    }

    /// Push a new local-variable scope. Use `pop_scope` to remove every binding
    /// introduced since the matching push.
    pub fn push_scope(&mut self) {
        self.local_scopes.push(yelang_arena::FxHashMap::new());
    }

    /// Pop the innermost local-variable scope, discarding all bindings that were
    /// introduced inside it.
    pub fn pop_scope(&mut self) {
        self.local_scopes.pop();
    }

    /// Push a local variable into the innermost scope.
    pub fn push_local(&mut self, name: Symbol, pat_id: PatId) {
        if let Some(scope) = self.local_scopes.last_mut() {
            scope.insert(name, pat_id);
        }
    }

    /// Look up a local variable, searching from the innermost scope outward.
    pub fn local(&self, name: Symbol) -> Option<PatId> {
        self.local_scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(&name).copied())
    }
}

/// Lower an entire AST `Program` into a HIR `Crate`.
pub fn lower_crate(program: &Program, resolved: &ResolvedCrate, interner: &Interner) -> Crate {
    let mut ctx = LoweringContext::new(interner, resolved);

    for item in &program.items {
        let _ = crate::lowering::item::lower_item(&mut ctx, item);
    }

    ctx.crate_hir
}

/// Look up the `DefId` for an AST item within the current module.
/// Matches by parent module, name, and kind.
pub(crate) fn lookup_item_def_id(ctx: &LoweringContext, item: &AstItem) -> Option<DefId> {
    let expected_kind = item_def_kind(&item.kind)?;
    let expected_name = item_name(item)?;
    ctx.resolved
        .definitions
        .iter_enumerated()
        .find(|(_, def)| {
            def.parent == Some(ctx.current_module)
                && def.name == expected_name
                && def.kind == expected_kind
        })
        .map(|(id, _)| id)
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
