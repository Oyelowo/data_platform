/*! TypeckResults — stores the inferred types for a function body. */

use yelang_arena::{DefId, FxHashMap};
use yelang_hir::ids::{ExprId, PatId};
use yelang_ty::ty::TyId;

use crate::autoderef::Adjustment;

/// The resolved origin of a method call.
///
/// This is recorded for every `Expr::MethodCall` so that later phases (QIR
/// lowering, codegen) can dispatch by `DefId` instead of by method name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MethodResolution {
    /// The trait the method came from, if any. `None` for inherent or builtin
    /// methods.
    pub trait_def_id: Option<DefId>,
    /// The method item's `DefId`, if known. Builtin intercepts may leave this
    /// `None` when no real item exists yet.
    pub method_def_id: Option<DefId>,
    /// The impl block the method was defined in, if any. `None` for trait
    /// methods whose impl is selected later by the trait solver.
    pub impl_def_id: Option<DefId>,
}

/// The result of type-checking a function body.
///
/// Maps HIR node IDs to their inferred types.
#[derive(Debug, Clone)]
pub struct TypeckResults {
    /// Inferred type of each expression.
    pub expr_types: FxHashMap<ExprId, TyId>,
    /// Inferred type of each pattern.
    pub pat_types: FxHashMap<PatId, TyId>,
    /// Type of each local variable (from pattern bindings).
    pub local_types: FxHashMap<PatId, TyId>,
    /// Receiver adjustments discovered for each method-call expression.
    pub expr_adjustments: FxHashMap<ExprId, Vec<Adjustment>>,
    /// Method-call resolutions keyed by the method-call expression id.
    pub method_resolutions: FxHashMap<ExprId, MethodResolution>,
    /// The function's definition id.
    pub def_id: DefId,
}

impl TypeckResults {
    pub fn new(def_id: DefId) -> Self {
        Self {
            expr_types: FxHashMap::new(),
            pat_types: FxHashMap::new(),
            local_types: FxHashMap::new(),
            expr_adjustments: FxHashMap::new(),
            method_resolutions: FxHashMap::new(),
            def_id,
        }
    }

    /// Record the resolved origin of a method call.
    pub fn record_method_resolution(
        &mut self,
        expr_id: ExprId,
        resolution: MethodResolution,
    ) {
        self.method_resolutions.insert(expr_id, resolution);
    }

    /// Look up the resolved origin of a method call.
    pub fn method_resolution(&self, expr_id: ExprId) -> Option<&MethodResolution> {
        self.method_resolutions.get(&expr_id)
    }

    pub fn expr_ty(&self, expr_id: ExprId) -> Option<TyId> {
        self.expr_types.get(&expr_id).copied()
    }

    pub fn pat_ty(&self, pat_id: PatId) -> Option<TyId> {
        self.pat_types.get(&pat_id).copied()
    }

    pub fn local_ty(&self, pat_id: PatId) -> Option<TyId> {
        self.local_types.get(&pat_id).copied()
    }

    pub fn expr_adjustments(&self, expr_id: ExprId) -> &[Adjustment] {
        self.expr_adjustments
            .get(&expr_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }
}
