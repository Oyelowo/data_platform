/*! TypeckResults — stores the inferred types for a function body. */

use yelang_arena::{DefId, FxHashMap};
use yelang_hir::ids::{ExprId, PatId};
use yelang_ty::ty::Ty;

/// The result of type-checking a function body.
///
/// Maps HIR node IDs to their inferred types.
#[derive(Debug, Clone)]
pub struct TypeckResults<'tcx> {
    /// Inferred type of each expression.
    pub expr_types: FxHashMap<ExprId, Ty<'tcx>>,
    /// Inferred type of each pattern.
    pub pat_types: FxHashMap<PatId, Ty<'tcx>>,
    /// Type of each local variable (from pattern bindings).
    pub local_types: FxHashMap<PatId, Ty<'tcx>>,
    /// The function's definition id.
    pub def_id: DefId,
}

impl<'tcx> TypeckResults<'tcx> {
    pub fn new(def_id: DefId) -> Self {
        Self {
            expr_types: FxHashMap::new(),
            pat_types: FxHashMap::new(),
            local_types: FxHashMap::new(),
            def_id,
        }
    }

    pub fn expr_ty(&self, expr_id: ExprId) -> Option<Ty<'tcx>> {
        self.expr_types.get(&expr_id).copied()
    }

    pub fn pat_ty(&self, pat_id: PatId) -> Option<Ty<'tcx>> {
        self.pat_types.get(&pat_id).copied()
    }

    pub fn local_ty(&self, pat_id: PatId) -> Option<Ty<'tcx>> {
        self.local_types.get(&pat_id).copied()
    }
}
