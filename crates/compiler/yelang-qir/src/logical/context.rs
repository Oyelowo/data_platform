//! Extraction context.

use yelang_arena::{DefId, FxHashMap};

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
    pub pats: &'a slotmap::SlotMap<yelang_thir::ThirPatId, yelang_thir::ThirPat>,
    pub stmts: &'a slotmap::SlotMap<yelang_thir::ThirStmtId, yelang_thir::ThirStmt>,
}

/// Context for lowering a THIR body to LIR.
pub struct ExtractCtxt<'a> {
    pub tcx: &'a yelang_tycheck::tcx::TyCtxt,
    pub thir: ThirView<'a>,
    pub results: &'a yelang_tycheck::typeck_results::TypeckResults,
    pub lang_traits: LangTraits,
    pub queryable_methods: FxHashMap<DefId, QueryableMethodInfo>,
    pub aggregate_impls: FxHashMap<DefId, AggregateImplInfo>,
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
            queryable_methods: FxHashMap::default(),
            aggregate_impls: FxHashMap::default(),
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
        // TODO(phase3): scan the Queryable trait and its impls for @intrinsic bodies.
        Ok(())
    }
}
