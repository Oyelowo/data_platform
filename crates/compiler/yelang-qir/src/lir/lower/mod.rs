//! Main HIR -> LIR lowering driver.
//!
//! Lowering is trait-driven: it reads `yelang_tycheck::typeck_results` to learn
//! which trait/method each HIR call resolved to. There is no method-name
//! pattern matching.

pub mod aggregate;
pub mod closure;
pub mod expr;
pub mod links;
pub mod method;
pub mod query;
pub mod queryable;
pub mod selector;

use yelang_arena::{DefId, FxHashMap};
use yelang_hir::ids::{BodyId, ExprId, PatId, QueryId};
use yelang_resolve::lang_items::LangItem;
use yelang_tycheck::tcx::TyCtxt;
use yelang_tycheck::typeck_results::{MethodResolution, TypeckResults};

use crate::errors::LoweringError;
use crate::expr::AggregateClass;
use crate::ids::{BinderId, LirId};
use crate::lir::plan::LogicalPlan;
use crate::lir::lower::queryable::QueryableMethod;

/// Context shared while lowering a single function body.
pub struct LoweringCtxt<'a> {
    pub tcx: &'a TyCtxt,
    pub body_id: BodyId,
    pub results: &'a TypeckResults,
    pub lang_traits: LangTraits,
    /// Maps an aggregate marker type or sugar method `DefId` to its
    /// `AggregateClass`. This is a pragmatic stand-in until the compiler can
    /// evaluate `Aggregate::classify()` directly from the trait impl.
    pub aggregate_classes: FxHashMap<DefId, AggregateClass>,
    /// Maps a `Queryable` trait method `DefId` to the operator it lowers to.
    pub queryable_methods: FxHashMap<DefId, QueryableMethod>,
    /// Binder scope stack mapping HIR pattern ids to pipeline binder ids.
    binder_scopes: Vec<FxHashMap<PatId, BinderId>>,
    /// Local variable values that should be inlined as QExpr fragments.
    ///
    /// Used for `let`-bound sources in query context (e.g. `let users = [1,2,3]`).
    local_values: FxHashMap<PatId, crate::ids::QExprId>,
}

impl<'a> LoweringCtxt<'a> {
    pub fn new(tcx: &'a TyCtxt, body_id: BodyId, results: &'a TypeckResults) -> Self {
        Self {
            tcx,
            body_id,
            results,
            lang_traits: LangTraits::load(tcx),
            aggregate_classes: FxHashMap::default(),
            queryable_methods: FxHashMap::default(),
            binder_scopes: vec![FxHashMap::default()],
            local_values: FxHashMap::default(),
        }
    }

    pub fn with_aggregate_class(mut self, def_id: DefId, class: AggregateClass) -> Self {
        self.aggregate_classes.insert(def_id, class);
        self
    }

    pub fn with_queryable_method(mut self, def_id: DefId, method: QueryableMethod) -> Self {
        self.queryable_methods.insert(def_id, method);
        self
    }

    pub fn krate(&self) -> &yelang_hir::crate_data::Crate {
        self.tcx.crate_hir()
    }

    /// Push a new binder scope.
    pub fn push_binder_scope(&mut self) {
        self.binder_scopes.push(FxHashMap::default());
    }

    /// Pop the innermost binder scope.
    pub fn pop_binder_scope(&mut self) {
        self.binder_scopes.pop();
    }

    /// Register a HIR pattern as a pipeline binder.
    pub fn insert_binder(&mut self, pat_id: PatId, binder: BinderId) {
        if let Some(scope) = self.binder_scopes.last_mut() {
            scope.insert(pat_id, binder);
        }
    }

    /// Look up the pipeline binder for a HIR local pattern.
    pub fn lookup_binder(&self, pat_id: PatId) -> Option<BinderId> {
        for scope in self.binder_scopes.iter().rev() {
            if let Some(&binder) = scope.get(&pat_id) {
                return Some(binder);
            }
        }
        None
    }

    /// Register a local variable initializer so that references to the variable
    /// can be inlined into QExpr instead of being treated as pipeline rows.
    pub fn insert_local_value(&mut self, pat_id: PatId, expr: crate::ids::QExprId) {
        self.local_values.insert(pat_id, expr);
    }

    /// Look up the inlined QExpr for a local variable, if any.
    pub fn lookup_local_value(&self, pat_id: PatId) -> Option<crate::ids::QExprId> {
        self.local_values.get(&pat_id).copied()
    }

    /// Populate the `queryable_methods` and `aggregate_classes` tables by
    /// introspecting the real `Queryable` and `Aggregate` lang-item definitions
    /// loaded from `stdlib/core/src/*.ye`.
    pub fn populate_stdlib_tables(&mut self) -> Result<(), crate::errors::LoweringError> {
        let (methods, classes) = crate::lir::stdlib::build_tables(self.tcx)?;
        // Manual overrides (used by unit tests) take precedence.
        for (k, v) in methods {
            self.queryable_methods.entry(k).or_insert(v);
        }
        for (k, v) in classes {
            self.aggregate_classes.entry(k).or_insert(v);
        }
        Ok(())
    }

    /// Look up the aggregate class for a marker/method def, if registered.
    pub fn aggregate_class(&self, def_id: DefId) -> Option<AggregateClass> {
        crate::lir::stdlib::aggregate_class(self.tcx, &self.aggregate_classes, def_id)
    }

    /// Look up the queryable operator kind for a method def, if registered.
    pub fn queryable_method(&self, def_id: DefId) -> Option<QueryableMethod> {
        self.queryable_methods.get(&def_id).copied()
    }
}

/// Walk the function body and inline any `let`-bound initializers that appear
/// before the query.  This lets `from users@u` work when `users` is a local
/// array literal or other constant expression.
pub fn populate_local_values(
    plan: &mut LogicalPlan,
    ctx: &mut LoweringCtxt<'_>,
    body_id: BodyId,
) -> Result<(), LoweringError> {
    let body = match ctx.krate().body(body_id) {
        Some(b) => b,
        None => return Ok(()),
    };
    let body_expr = match ctx.krate().expr(body.value) {
        Some(e) => e,
        None => return Ok(()),
    };

    let stmts = match body_expr {
        yelang_hir::hir::expr::Expr::Block { block } => block.stmts.clone(),
        _ => return Ok(()),
    };

    for stmt_id in stmts {
        let stmt = ctx
            .krate()
            .stmt(stmt_id)
            .ok_or(LoweringError::UnsupportedExpr)?;
        let (pat, init) = match stmt {
            yelang_hir::hir::core::Stmt::Let {
                pat,
                init: Some(init),
                ..
            } => (*pat, *init),
            _ => continue,
        };
        let expr = expr::lower_hir_expr(plan, ctx, init)?;
        ctx.insert_local_value(pat, expr);
    }

    Ok(())
}

/// Lower a typed HIR query into a logical plan.
///
/// This is the main entry point for Phase I. The physical planner and executor
/// consume the returned `LogicalPlan`.
pub fn lower_query(
    plan: &mut LogicalPlan,
    ctx: &mut LoweringCtxt<'_>,
    query_id: QueryId,
) -> Result<LirId, LoweringError> {
    let query = ctx
        .krate()
        .query(query_id)
        .cloned()
        .ok_or(LoweringError::UnsupportedClause)?;
    query::lower_query(plan, ctx, &query)
}

/// Lower an arbitrary HIR expression that appears in query context.
pub fn lower_expr(
    plan: &mut LogicalPlan,
    ctx: &mut LoweringCtxt<'_>,
    expr_id: yelang_hir::ids::ExprId,
) -> Result<crate::ids::QExprId, LoweringError> {
    expr::lower_hir_expr(plan, ctx, expr_id)
}

/// Resolve a method call to its trait/method DefIds.
pub fn resolve_method(ctx: &LoweringCtxt<'_>, expr_id: ExprId) -> Option<MethodRes> {
    ctx.results.method_resolution(expr_id).map(|res| MethodRes::from_resolution(res))
}

/// Check whether a method resolution is through a given lang-item trait.
pub fn is_lang_trait(ctx: &LoweringCtxt<'_>, trait_def: DefId, lang: LangItem) -> bool {
    ctx.tcx.lang_item(lang) == Some(trait_def)
}

/// Minimal method-resolution fact used by lowering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MethodRes {
    pub trait_def_id: Option<DefId>,
    pub method_def_id: Option<DefId>,
    pub impl_def_id: Option<DefId>,
}

impl MethodRes {
    pub fn from_resolution(res: &MethodResolution) -> Self {
        Self {
            trait_def_id: res.trait_def_id,
            method_def_id: res.method_def_id,
            impl_def_id: res.impl_def_id,
        }
    }
}

/// Return the lang-item trait names we care about.
#[derive(Clone, Copy, Debug, Default)]
pub struct LangTraits {
    pub iterator: Option<DefId>,
    pub into_iter: Option<DefId>,
    pub queryable: Option<DefId>,
    pub aggregate: Option<DefId>,
}

impl LangTraits {
    pub fn load(tcx: &TyCtxt) -> Self {
        Self {
            iterator: tcx.lang_item(LangItem::Iterator),
            into_iter: tcx.lang_item(LangItem::IntoIterator),
            queryable: tcx.lang_item(LangItem::Queryable),
            aggregate: tcx.lang_item(LangItem::Aggregate),
        }
    }
}
