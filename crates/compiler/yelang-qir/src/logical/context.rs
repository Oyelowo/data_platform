//! Extraction context.

use yelang_arena::{DefId, FxHashMap};
use yelang_hir::hir::core::{ImplItemKind, TraitItemKind};
use yelang_hir::hir::expr::Expr;
use yelang_hir::res::Res;

use crate::errors::LoweringError;

/// Lang-item trait def ids discovered from the type context.
#[derive(Debug, Clone, Copy, Default)]
pub struct LangTraits {
    pub queryable: Option<DefId>,
    pub aggregate: Option<DefId>,
    pub iterator: Option<DefId>,
    pub into_iterator: Option<DefId>,
    pub from_iterator: Option<DefId>,
}

/// ADT types that are known to implement `Queryable`.
#[derive(Debug, Clone, Copy, Default)]
pub struct QueryableAdts {
    /// `QueryArray<T>` wrapper.
    pub query_array: Option<DefId>,
    /// Built-in `Array<T>` type (transitional impl).
    pub array: Option<DefId>,
}

/// Information about a `Queryable` method derived from the trait definition.
#[derive(Debug, Clone)]
pub struct QueryableMethodInfo {
    /// DefId of the trait method.
    pub def_id: DefId,
    /// Position of the `self` parameter in the call argument list.
    pub self_index: usize,
    /// Map from formal parameter name to argument index (after self).
    pub arg_index: FxHashMap<yelang_interner::Symbol, usize>,
    /// Recognized intrinsic, if any.
    pub intrinsic: Option<super::intrinsic::QueryableIntrinsic>,
    /// For sugar methods (`sum`, `count`, ...), the DefId of the aggregate
    /// marker type (e.g. `Sum`, `Count`).
    pub sugar_marker: Option<DefId>,
}

/// Information about a selected `Aggregate` impl.
#[derive(Debug, Clone)]
pub struct AggregateImplInfo {
    pub impl_def: DefId,
    pub agg_def: DefId,
    pub input_ty: yelang_ty::ty::TyId,
    pub acc_ty: yelang_ty::ty::TyId,
    pub out_ty: yelang_ty::ty::TyId,
    pub init: DefId,
    pub step: DefId,
    pub merge: DefId,
    pub finish: DefId,
    pub class: crate::expr::AggregateClass,
}

/// Borrowed view of all THIR data needed by the extractor.
pub struct ThirView<'a> {
    pub bodies: &'a yelang_thir::body::ThirBodies,
    pub exprs: &'a slotmap::SlotMap<yelang_thir::ThirExprId, yelang_thir::ThirExpr>,
    pub expr_tys: &'a slotmap::SecondaryMap<yelang_thir::ThirExprId, yelang_ty::ty::TyId>,
    pub pats: &'a slotmap::SlotMap<yelang_thir::ThirPatId, yelang_thir::ThirPat>,
    pub stmts: &'a slotmap::SlotMap<yelang_thir::ThirStmtId, yelang_thir::ThirStmt>,
    /// Mapping from HIR pattern ids to THIR pattern ids for the current body.
    pub local_pats: &'a yelang_arena::FxHashMap<yelang_hir::ids::PatId, yelang_thir::ThirPatId>,
}

/// Context for lowering a THIR body to LIR.
pub struct ExtractCtxt<'a> {
    pub tcx: &'a yelang_tycheck::tcx::TyCtxt,
    pub thir: ThirView<'a>,
    pub results: &'a yelang_tycheck::typeck_results::TypeckResults,
    pub lang_traits: LangTraits,
    pub queryable_adts: QueryableAdts,
    pub queryable_methods: FxHashMap<DefId, QueryableMethodInfo>,
    pub aggregate_impls: FxHashMap<DefId, AggregateImplInfo>,
    /// Name symbol of the `aggregate` method, discovered while scanning the
    /// `Queryable` trait. Used to expand sugar method calls.
    aggregate_method_name: Option<yelang_interner::Symbol>,
    /// Stack of binder scopes. Each scope maps a THIR pattern id to the QIR
    /// binder id used for pipeline columns and closure parameters.
    binder_scopes: Vec<FxHashMap<yelang_thir::ThirPatId, crate::ids::BinderId>>,
    /// HIR pattern ids that reference `let`-bound local values, mapped to the
    /// QExpr fragment that should be inlined when the variable is referenced
    /// from query syntax (e.g. `from xs@x` where `xs` is a local array).
    pub hir_local_values: FxHashMap<yelang_hir::ids::PatId, crate::ids::QExprId>,
    /// Stack of binder scopes keyed by HIR pattern id. Used by query-syntax
    /// lowering for `from`, `group by`, and nested closures.
    hir_binder_scopes: Vec<FxHashMap<yelang_hir::ids::PatId, crate::ids::BinderId>>,
    /// QIR expressions that a binder is bound to in the current THIR body.
    /// This is used to inline local array sources when a subplan is extracted
    /// from its surrounding `let` context.
    binder_local_values: FxHashMap<crate::ids::BinderId, crate::ids::QExprId>,
}

impl<'a> ExtractCtxt<'a> {
    pub fn new(
        tcx: &'a yelang_tycheck::tcx::TyCtxt,
        thir: ThirView<'a>,
        results: &'a yelang_tycheck::typeck_results::TypeckResults,
    ) -> Result<Self, LoweringError> {
        let mut ctx = Self {
            tcx,
            thir,
            results,
            lang_traits: LangTraits::default(),
            queryable_adts: QueryableAdts::default(),
            queryable_methods: FxHashMap::default(),
            aggregate_impls: FxHashMap::default(),
            aggregate_method_name: None,
            binder_scopes: vec![FxHashMap::default()],
            hir_local_values: FxHashMap::default(),
            hir_binder_scopes: vec![FxHashMap::default()],
            binder_local_values: FxHashMap::default(),
        };
        ctx.discover_lang_items();
        ctx.discover_queryable_methods()?;
        Ok(ctx)
    }

    fn discover_lang_items(&mut self) {
        use yelang_resolve::lang_items::LangItem;
        self.lang_traits.queryable = self.tcx.lang_item(LangItem::Queryable);
        self.lang_traits.aggregate = self.tcx.lang_item(LangItem::Aggregate);
        self.lang_traits.iterator = self.tcx.lang_item(LangItem::Iterator);
        self.lang_traits.into_iterator = self.tcx.lang_item(LangItem::IntoIterator);
        self.lang_traits.from_iterator = self.tcx.lang_item(LangItem::FromIterator);
    }

    fn discover_queryable_methods(&mut self) -> Result<(), LoweringError> {
        let Some(queryable_trait) = self.lang_traits.queryable else {
            // The `Queryable` lang item is not required for every program. If it
            // is missing, the extractor simply has no queryable methods to map.
            return Ok(());
        };

        let hir = self.tcx.crate_hir();
        let trait_data = self
            .tcx
            .trait_def(queryable_trait)
            .ok_or(LoweringError::UnsupportedExpr)?;

        // Collect impl bodies that implement the Queryable trait, keyed by the
        // impl item's name so they can be matched to the corresponding trait
        // method. We keep all candidate bodies and later prefer the one that
        // contains a recognized `@intrinsic(query_*, ...)` call. This avoids
        // placeholder impls (e.g. the transitional `impl Queryable<T> for Array<T>`)
        // shadowing the canonical `QueryArray<T>` impl.
        let mut impl_bodies: FxHashMap<
            yelang_interner::Symbol,
            Vec<yelang_hir::ids::BodyId>,
        > = FxHashMap::default();
        for imp in &hir.impls {
            let Some(of_trait) = &imp.of_trait else { continue };
            if self.res_to_def_id(&of_trait.path) != Some(queryable_trait) {
                continue;
            }
            // Remember the ADT that this impl is for (QueryArray<T>, Array<T>, ...).
            if let Some(adt_def_id) = self.adt_def_id(imp.self_ty) {
                let def = hir.definition(adt_def_id);
                let name = def.map(|d| self.resolve_symbol(d.name));
                match name {
                    Some(Some("QueryArray")) => self.queryable_adts.query_array = Some(adt_def_id),
                    Some(Some("Array")) => self.queryable_adts.array = Some(adt_def_id),
                    _ => {}
                }
            }
            for item in &imp.items {
                if let ImplItemKind::Fn { body, .. } = &item.kind {
                    impl_bodies
                        .entry(item.ident.symbol)
                        .or_default()
                        .push(*body);
                }
            }
        }

        for item in &trait_data.items {
            let method_def_id = item.def_id();
            let ident = item.ident();

            // Prefer an impl body that contains a recognized `@intrinsic` hook;
            // that is the canonical source of truth for how the method is lowered.
            // If no impl body has an intrinsic, fall back to the first available
            // impl body, then to the trait default body.
            let mut body_id: Option<yelang_hir::ids::BodyId> = None;
            if let Some(candidates) = impl_bodies.get(&ident.symbol) {
                body_id = candidates
                    .iter()
                    .copied()
                    .find(|&b| self.find_top_level_intrinsic(b).is_some())
                    .or_else(|| candidates.first().copied());
            }
            if body_id.is_none() {
                if let Some(trait_hir) = hir.traits.get(queryable_trait).and_then(|t| t.as_ref()) {
                    for trait_item in &trait_hir.items {
                        if trait_item.def_id == method_def_id {
                            if let TraitItemKind::Fn { default, .. } = &trait_item.kind {
                                body_id = *default;
                            }
                            break;
                        }
                    }
                }
            }

            let signature = self.method_signature(method_def_id, body_id);

            // Discover the intrinsic. Sugar methods (`sum`, `count`, ...) have a
            // body of the form `self.aggregate(Marker {})`; methods like `take`
            // contain a direct `@intrinsic(query_*, ...)` call. Either shape can
            // appear in a trait default body or in an impl body.
            let intrinsic = if let Some(body) = body_id {
                self.find_sugar_intrinsic(body)
                    .or_else(|| self.find_top_level_intrinsic(body))
            } else {
                None
            };

            let is_sugar_aggregate = intrinsic
                == Some(super::intrinsic::QueryableIntrinsic::Aggregate)
                && self.is_sugar_aggregate_call_body(body_id);
            let sugar_marker = if is_sugar_aggregate {
                self.sugar_marker_def_id(method_def_id)
            } else {
                None
            };

            self.queryable_methods.insert(
                method_def_id,
                QueryableMethodInfo {
                    def_id: method_def_id,
                    self_index: 0,
                    arg_index: signature,
                    intrinsic,
                    sugar_marker,
                },
            );

            // Remember the name of the `aggregate` method so extraction can
            // expand sugar method calls (`sum`, `count`, ...).
            if intrinsic == Some(super::intrinsic::QueryableIntrinsic::Aggregate) {
                self.aggregate_method_name = Some(ident.symbol);
            }
        }

        Ok(())
    }

    /// Look up information about a `Queryable` method by its item `DefId`.
    pub fn queryable_method_info(&self, def_id: DefId) -> Option<&QueryableMethodInfo> {
        self.queryable_methods.get(&def_id)
    }

    /// Return the inferred type of a THIR expression.
    pub fn thir_expr_ty(&self, expr_id: yelang_thir::ThirExprId) -> Option<yelang_ty::ty::TyId> {
        self.thir.expr_tys.get(expr_id).copied()
    }

    /// Look up information about a previously resolved aggregate impl.
    pub fn aggregate_impl_info(&self, config_def_id: DefId) -> Option<&AggregateImplInfo> {
        self.aggregate_impls.get(&config_def_id)
    }

    /// Cache information about a resolved aggregate impl.
    pub fn insert_aggregate_impl_info(&mut self, info: AggregateImplInfo) {
        self.aggregate_impls.insert(info.agg_def, info);
    }

    /// Resolve a HIR path result to a `DefId`, when possible.
    /// If a HIR type resolves to an ADT, return its `DefId`.
    fn adt_def_id(&self, ty: yelang_hir::ids::HirTyId) -> Option<DefId> {
        let hir_ty = self.tcx.crate_hir().ty(ty)?;
        match hir_ty {
            yelang_hir::hir::ty::Ty::Path { res, .. } => self.res_to_def_id(res),
            _ => None,
        }
    }

    fn res_to_def_id(&self, res: &Res) -> Option<DefId> {
        match res {
            Res::Def { def_id } => Some(*def_id),
            _ => None,
        }
    }

    /// Resolve a symbol to its textual form.
    fn resolve_symbol(&self, sym: yelang_interner::Symbol) -> Option<&str> {
        self.tcx.resolve_symbol(sym)
    }

    /// Build a map from formal parameter name to argument index. The `self`
    /// parameter is always at index 0.
    fn method_signature(
        &self,
        _method_def_id: DefId,
        body_id: Option<yelang_hir::ids::BodyId>,
    ) -> FxHashMap<yelang_interner::Symbol, usize> {
        let mut map = FxHashMap::default();
        let Some(body_id) = body_id else { return map };
        let Some(body) = self.tcx.crate_hir().body(body_id) else {
            return map;
        };
        for (idx, param) in body.params.iter().enumerate() {
            let Some(pat) = self.tcx.crate_hir().pat(param.pat) else { continue };
            let name = match pat {
                yelang_hir::hir::pat::Pat::Binding { name, .. } => *name,
                _ => continue,
            };
            map.insert(name, idx);
        }
        map
    }

    /// Find the first `@intrinsic(query_*, ...)` call in a method body and
    /// return the corresponding `QueryableIntrinsic`.
    ///
    /// The stdlib currently writes these as `@intrinsic(query_map(self.plan, f))`,
    /// i.e. the intrinsic name is `intrinsic` and the first argument is a call
    /// to the placeholder function `query_map`. We therefore look at the first
    /// argument and extract the callee's name.
    fn find_top_level_intrinsic(
        &self,
        body_id: yelang_hir::ids::BodyId,
    ) -> Option<super::intrinsic::QueryableIntrinsic> {
        let hir = self.tcx.crate_hir();
        let body = hir.body(body_id)?;
        self.find_intrinsic_in_expr(body.value)
    }

    fn find_intrinsic_in_expr(
        &self,
        expr_id: yelang_hir::ids::ExprId,
    ) -> Option<super::intrinsic::QueryableIntrinsic> {
        let hir = self.tcx.crate_hir();
        let expr = hir.expr(expr_id)?;
        match expr {
            Expr::Block { block } => {
                for stmt_id in &block.stmts {
                    let stmt = hir.stmt(*stmt_id)?;
                    if let yelang_hir::hir::core::Stmt::Let { init, .. } = stmt {
                        if let Some(init_expr) = init {
                            if let Some(i) = self.find_intrinsic_in_expr(*init_expr) {
                                return Some(i);
                            }
                        }
                    }
                }
                block
                    .expr
                    .and_then(|e| self.find_intrinsic_in_expr(e))
            }
            Expr::Intrinsic { name, args } => {
                let name_str = self.resolve_symbol(name.symbol);
                if name_str == Some("intrinsic") {
                    let first_arg = args.first()?;
                    let callee_name = self.callee_name(*first_arg)?;
                    return self.resolve_symbol(callee_name)
                        .and_then(super::intrinsic::QueryableIntrinsic::from_str);
                }
                None
            }
            Expr::Struct { .. } | Expr::Tuple { .. } | Expr::Path { .. } | Expr::Call { .. } => None,
            _ => None,
        }
    }

    /// Extract the name symbol of the callee for a call expression or path.
    fn callee_name(&self, expr_id: yelang_hir::ids::ExprId) -> Option<yelang_interner::Symbol> {
        let hir = self.tcx.crate_hir();
        let expr = hir.expr(expr_id)?;
        match expr {
            Expr::Call { func, .. } => self.callee_name(*func),
            Expr::Path { res } => match res {
                Res::Def { def_id } => {
                    let def = hir.definition(*def_id)?;
                    Some(def.name)
                }
                _ => None,
            },
            _ => None,
        }
    }

    /// Recognize sugar method bodies of the form `self.aggregate(Marker {})`.
    /// Returns `Some(Aggregate)` so that `sum`, `count`, etc. are lowered like
    /// `aggregate`.
    fn find_sugar_intrinsic(
        &self,
        body_id: yelang_hir::ids::BodyId,
    ) -> Option<super::intrinsic::QueryableIntrinsic> {
        if self.is_sugar_aggregate_call_body(Some(body_id)) {
            Some(super::intrinsic::QueryableIntrinsic::Aggregate)
        } else {
            None
        }
    }

    /// True if the given body is a sugar aggregate call `self.aggregate(Marker {})`.
    fn is_sugar_aggregate_call_body(
        &self,
        body_id: Option<yelang_hir::ids::BodyId>,
    ) -> bool {
        let Some(body_id) = body_id else { return false };
        let hir = self.tcx.crate_hir();
        let Some(body) = hir.body(body_id) else { return false };
        let self_pat = body.params.first().map(|p| p.pat);
        self.is_sugar_aggregate_call(body.value, self_pat)
    }

    fn sugar_marker_def_id(&self, method_def_id: DefId) -> Option<DefId> {
        let hir = self.tcx.crate_hir();
        let def = hir.definition(method_def_id)?;
        let name = self.resolve_symbol(def.name)?;
        let marker_name = match name {
            "sum" => "Sum",
            "count" => "Count",
            "avg" => "Avg",
            "min" => "Min",
            "max" => "Max",
            _ => return None,
        };
        hir.items.iter_enumerated().find_map(|(def_id, opt_item)| {
            let item = opt_item.as_ref()?;
            if self.resolve_symbol(item.ident.symbol) == Some(marker_name) {
                Some(def_id)
            } else {
                None
            }
        })
    }

    fn is_sugar_aggregate_call(
        &self,
        expr_id: yelang_hir::ids::ExprId,
        self_pat: Option<yelang_hir::ids::PatId>,
    ) -> bool {
        let hir = self.tcx.crate_hir();
        let Some(expr) = hir.expr(expr_id) else { return false };
        let body_expr = match expr {
            Expr::Block { block } => {
                if let Some(tail) = block.expr {
                    tail
                } else {
                    return false;
                }
            }
            _ => expr_id,
        };
        let Some(body_expr) = hir.expr(body_expr) else { return false };
        let Expr::MethodCall { receiver, method, args, .. } = body_expr else {
            return false;
        };
        if self.resolve_symbol(method.symbol) != Some("aggregate") {
            return false;
        }
        let Some(receiver_expr) = hir.expr(*receiver) else { return false };
        let is_self_receiver = match receiver_expr {
            Expr::Path { res: Res::SelfVal { .. } } => true,
            Expr::Path { res: Res::Local { pat_id } } => Some(*pat_id) == self_pat,
            _ => false,
        };
        if !is_self_receiver {
            return false;
        }
        // The argument should be a marker struct literal like `Sum {}`.
        args.len() == 1
    }

    /// If `ty` is a known `Queryable` ADT (`QueryArray<T>` or `Array<T>`),
    /// return the element type `T`.
    pub fn queryable_element_ty(&self, ty: yelang_ty::ty::TyId) -> Option<yelang_ty::ty::TyId> {
        let yelang_ty::ty::Ty::Adt(adt_def, args) = self.tcx.interner().ty(ty) else {
            return None;
        };
        let is_queryable = self.queryable_adts.query_array == Some(adt_def.def_id)
            || self.queryable_adts.array == Some(adt_def.def_id);
        if !is_queryable {
            return None;
        }
        args.first().map(|arg| arg.expect_type())
    }

    /// Return the name of the `aggregate` method as discovered from the
    /// `Queryable` trait, if any.
    pub fn aggregate_method_name(&self) -> Option<yelang_interner::Symbol> {
        self.aggregate_method_name
    }

    /// Allocate a fresh binder and register it for the given THIR pattern.
    pub fn insert_binder(
        &mut self,
        pat_id: yelang_thir::ThirPatId,
        binder: crate::ids::BinderId,
    ) {
        if let Some(scope) = self.binder_scopes.last_mut() {
            scope.insert(pat_id, binder);
        }
    }

    /// Look up the QIR binder for a THIR pattern id.
    pub fn lookup_binder(&self, pat_id: yelang_thir::ThirPatId) -> Option<crate::ids::BinderId> {
        for scope in self.binder_scopes.iter().rev() {
            if let Some(binder) = scope.get(&pat_id) {
                return Some(*binder);
            }
        }
        None
    }

    /// Push a new binder scope.
    pub fn push_binder_scope(&mut self) {
        self.binder_scopes.push(FxHashMap::default());
    }

    /// Pop the current binder scope.
    pub fn pop_binder_scope(&mut self) {
        self.binder_scopes.pop();
    }

    /// Register a HIR pattern as an inlined local value (e.g. a `let`-bound
    /// source collection).
    pub fn insert_hir_local_value(
        &mut self,
        pat_id: yelang_hir::ids::PatId,
        expr: crate::ids::QExprId,
    ) {
        self.hir_local_values.insert(pat_id, expr);
    }

    /// Look up the QExpr fragment for a HIR local variable, if any.
    pub fn lookup_hir_local_value(
        &self,
        pat_id: yelang_hir::ids::PatId,
    ) -> Option<crate::ids::QExprId> {
        self.hir_local_values.get(&pat_id).copied()
    }

    /// Register a HIR pattern as a pipeline row binder.
    pub fn insert_hir_binder(
        &mut self,
        pat_id: yelang_hir::ids::PatId,
        binder: crate::ids::BinderId,
    ) {
        if let Some(scope) = self.hir_binder_scopes.last_mut() {
            scope.insert(pat_id, binder);
        }
    }

    /// Look up the QIR binder for a HIR pattern id.
    pub fn lookup_hir_binder(
        &self,
        pat_id: yelang_hir::ids::PatId,
    ) -> Option<crate::ids::BinderId> {
        for scope in self.hir_binder_scopes.iter().rev() {
            if let Some(binder) = scope.get(&pat_id) {
                return Some(*binder);
            }
        }
        None
    }

    /// Register the QIR expression that a binder is bound to.
    pub fn insert_binder_local_value(
        &mut self,
        binder: crate::ids::BinderId,
        expr: crate::ids::QExprId,
    ) {
        self.binder_local_values.insert(binder, expr);
    }

    /// Look up the QIR expression that a binder is bound to, if any.
    pub fn lookup_binder_local_value(
        &self,
        binder: crate::ids::BinderId,
    ) -> Option<crate::ids::QExprId> {
        self.binder_local_values.get(&binder).copied()
    }

    /// Push a new HIR-pattern binder scope.
    pub fn push_hir_binder_scope(&mut self) {
        self.hir_binder_scopes.push(FxHashMap::default());
    }

    /// Pop the current HIR-pattern binder scope.
    pub fn pop_hir_binder_scope(&mut self) {
        self.hir_binder_scopes.pop();
    }

}
