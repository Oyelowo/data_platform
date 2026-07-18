/*! FnCtxt — the function body type-checking context.
 *
 * Owns the inference state, local variable scopes, and breakable scopes
 * for checking a single function body.
 */

use yelang_arena::{DefId, FxHashMap};
use yelang_ast::Label;
use yelang_hir::Crate as HirCrate;
use yelang_hir::ids::{ExprId, PatId};
use yelang_infer::error::TypeError;
use yelang_lexer::Span;
use yelang_ty::canonical::{CanonicalResponse, CanonicalVarValue};
use yelang_ty::fold::{TypeFoldable, TypeFolder, TypeSuperFoldable};
use yelang_ty::generic::GenericArg;
use yelang_ty::interner::Interner;
use yelang_ty::list::List;
use yelang_ty::predicate::{
    ParamEnv, Predicate, ProjectionPredicate, TraitPredicate, WellFormedPredicate,
};
use yelang_ty::primitive::{FloatTy, IntTy};
use yelang_ty::ty::{
    AdtDef, Const, ConstId, ConstVid, FloatVid, ImplPolarity, InferTy, IntVid, Mutability, Ty,
    TyId, TyVid, TypeAndMut,
};

use crate::lower_ctx::TyLowerCtxt;
use crate::tcx::TyCtxt;
use crate::typeck_results::TypeckResults;
use yelang_infer::context::InferCtxt;
use yelang_infer::type_variable::{FloatVarValue, IntVarValue, TypeVarValue};
use yelang_trait_solver::eval_ctxt::EvalCtxt;
use yelang_trait_solver::goal::Goal;
use yelang_trait_solver::response::Certainty;

/// A breakable scope for loop/break type checking.
#[derive(Debug, Clone)]
pub struct BreakableScope {
    pub label: Option<Label>,
    pub kind: BreakableKind,
    pub expr_ty: TyId,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakableKind {
    Loop,
    Block,
}

/// Why a trait obligation could not be proven.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObligationStatus {
    /// The solver found no applicable impl/assumption.
    NoSolution,
    /// The solver could not make progress due to unresolved inference variables
    /// or ambiguity between candidates.
    Ambiguous,
}

/// An obligation that the trait solver could not prove with certainty.
#[derive(Debug, Clone)]
pub struct UnprovenObligation {
    pub pred: Predicate,
    pub status: ObligationStatus,
}

/// The function body type-checking context.
pub struct FnCtxt<'a> {
    /// The global type context: interner, item tables, and HIR reference.
    pub tcx: &'a TyCtxt,
    /// The inference context.
    pub infer: InferCtxt,
    /// Collected results.
    pub results: TypeckResults,
    /// Local variable scope stack. Each frame is a map from PatId to type.
    pub local_scopes: Vec<FxHashMap<PatId, TyId>>,
    /// Breakable scope stack.
    pub breakable_scopes: Vec<BreakableScope>,
    /// The expected return type of the function.
    pub return_ty: TyId,
    /// The self type (if inside an impl).
    pub self_ty: Option<TyId>,
    /// Whether we're currently in an irrefutable pattern context.
    pub in_irrefutable_pat: bool,
    /// The parameter environment for proving trait obligations.
    pub param_env: ParamEnv,
    /// Accumulated trait/well-formedness obligations to prove at the end of
    /// the function body.
    pub obligations: Vec<Predicate>,
    /// Type errors discovered while checking this body.
    pub errors: Vec<(Span, TypeError)>,
}

impl<'a> FnCtxt<'a> {
    pub fn new(tcx: &'a TyCtxt, def_id: DefId, return_ty: TyId) -> Self {
        Self {
            tcx,
            infer: InferCtxt::new(),
            results: TypeckResults::new(def_id),
            local_scopes: vec![FxHashMap::new()],
            breakable_scopes: Vec::new(),
            return_ty,
            self_ty: None,
            in_irrefutable_pat: false,
            param_env: tcx.param_env(def_id),
            obligations: Vec::new(),
            errors: Vec::new(),
        }
    }

    // -----------------------------------------------------------------------
    // Type creation helpers
    // -----------------------------------------------------------------------

    pub fn mk_ty(&self, kind: Ty) -> TyId {
        self.tcx.interner().mk_ty(kind)
    }

    pub fn mk_tuple(&self, tys: &[TyId]) -> TyId {
        let args = self
            .tcx
            .interner()
            .mk_generic_args(&tys.iter().map(|&t| GenericArg::Type(t)).collect::<Vec<_>>());
        self.mk_ty(Ty::Tuple(args))
    }

    pub fn mk_unit(&self) -> TyId {
        self.mk_ty(Ty::Tuple(List::empty()))
    }

    pub fn mk_array(&self, ty: TyId, len: ConstId) -> TyId {
        self.mk_ty(Ty::Array(ty, len))
    }

    pub fn mk_slice(&self, ty: TyId) -> TyId {
        self.mk_ty(Ty::Slice(ty))
    }

    pub fn mk_ref(&self, ty: TyId, mutbl: Mutability) -> TyId {
        self.mk_ty(Ty::Ref(ty, mutbl))
    }

    pub fn mk_raw_ptr(&self, ty: TyId, mutbl: Mutability) -> TyId {
        self.mk_ty(Ty::RawPtr(TypeAndMut { ty, mutbl }))
    }

    pub fn mk_never(&self) -> TyId {
        self.mk_ty(Ty::Never)
    }

    pub fn mk_bool(&self) -> TyId {
        self.mk_ty(Ty::Bool)
    }

    pub fn mk_int(&self, it: IntTy) -> TyId {
        self.mk_ty(Ty::Int(it))
    }

    pub fn mk_uint(&self, ut: yelang_ty::primitive::UintTy) -> TyId {
        self.mk_ty(Ty::Uint(ut))
    }

    pub fn mk_float(&self, ft: FloatTy) -> TyId {
        self.mk_ty(Ty::Float(ft))
    }

    pub fn mk_char(&self) -> TyId {
        self.mk_ty(Ty::Char)
    }

    pub fn mk_str(&self) -> TyId {
        self.mk_ty(Ty::Str)
    }

    pub fn mk_error(&self) -> TyId {
        self.mk_ty(Ty::Error)
    }

    pub fn mk_adt(&self, def_id: DefId, args: List<GenericArg>) -> TyId {
        self.mk_ty(Ty::Adt(AdtDef { def_id }, args))
    }

    /// Create the prelude `Array<T>` type if the `Array` lang item is known.
    pub fn mk_array_ty(&self, elem: TyId) -> TyId {
        let Some(def_id) = self.tcx.lang_item(yelang_resolve::lang_items::LangItem::Array) else {
            return self.mk_error();
        };
        let args = self
            .tcx
            .interner()
            .mk_generic_args(&[GenericArg::Type(elem)]);
        self.mk_adt(def_id, args)
    }

    /// If `ty` is an `Array<T>`, `Slice<T>`, or an unresolved inference variable
    /// that can be unified with `Array<T>`, return the element type `T`.
    pub fn expect_array(&mut self, span: Span, ty: TyId) -> TyId {
        let interner = self.tcx.interner();
        match interner.ty(ty) {
            Ty::Adt(adt, args) => {
                if self
                    .tcx
                    .lang_item(yelang_resolve::lang_items::LangItem::Array)
                    == Some(adt.def_id)
                {
                    args.iter()
                        .next()
                        .map(|a| a.expect_type())
                        .unwrap_or_else(|| self.mk_error())
                } else {
                    self.report_type_error(
                        span,
                        TypeError::Custom(format!(
                            "expected an array type, found `{}`",
                            format_ty(self.tcx, ty)
                        )),
                    );
                    self.mk_error()
                }
            }
            Ty::Slice(elem) => elem,
            Ty::Array(elem, _) => elem,
            Ty::Infer(InferTy::TyVar(_)) => {
                let elem = self.new_ty_var();
                let array_ty = self.mk_array_ty(elem);
                if let Err(e) = self.eq(ty, array_ty) {
                    self.report_type_error(span, e);
                }
                elem
            }
            Ty::Error => self.mk_error(),
            _ => {
                self.report_type_error(
                    span,
                    TypeError::Custom(format!(
                        "expected an array type, found `{}`",
                        format_ty(self.tcx, ty)
                    )),
                );
                self.mk_error()
            }
        }
    }

    pub fn mk_fn_ptr(&self, inputs: List<GenericArg>, output: TyId) -> TyId {
        self.mk_ty(Ty::FnPtr(yelang_ty::ty::PolyFnSig {
            sig: yelang_ty::ty::FnSig {
                inputs,
                output,
                return_ty_infer: false,
            },
        }))
    }

    // -----------------------------------------------------------------------
    // Inference variable creation
    // -----------------------------------------------------------------------

    pub fn new_ty_var(&mut self) -> TyId {
        self.infer.new_ty_var(self.tcx.interner())
    }

    pub fn new_int_var(&mut self) -> TyId {
        self.infer.new_int_var(self.tcx.interner())
    }

    pub fn new_float_var(&mut self) -> TyId {
        self.infer.new_float_var(self.tcx.interner())
    }

    // -----------------------------------------------------------------------
    // Unification helpers
    // -----------------------------------------------------------------------

    pub fn eq(&mut self, a: TyId, b: TyId) -> Result<(), TypeError> {
        self.infer.eq(self.tcx.interner(), a, b)
    }

    pub fn demand_eq(&mut self, span: Span, expected: TyId, found: TyId) -> TyId {
        if let Err(e) = self.eq(expected, found) {
            self.report_type_error(span, e);
            expected
        } else {
            expected
        }
    }

    /// Record a type error to be reported later.
    pub fn report_type_error(&mut self, span: Span, err: TypeError) {
        self.errors.push((span, err));
    }

    /// Report a simple expected/found mismatch.
    pub fn report_mismatch(&mut self, span: Span, expected: TyId, found: TyId) {
        self.report_type_error(span, TypeError::Mismatch { expected, found });
    }

    /// Report an unproven or ambiguous trait obligation.
    pub fn report_obligation_error(&mut self, span: Span, obligation: UnprovenObligation) {
        let msg = format_unproven_obligation(self.tcx, &obligation);
        self.report_type_error(span, TypeError::Custom(msg));
    }

    /// Create a fresh substitution that maps an item's generic parameters to
    /// new inference variables.
    pub fn fresh_substitution_for_generics(
        &mut self,
        def_id: yelang_arena::DefId,
    ) -> yelang_ty::generic::Substitution {
        use yelang_ty::generic::Substitution;

        let mut args = Vec::new();
        if let Some(generics) = self.tcx.generics_of(def_id) {
            for param in &generics.params {
                match param.kind {
                    crate::tcx::GenericParamKind::Type => {
                        args.push(yelang_ty::generic::GenericArg::Type(self.new_ty_var()));
                    }
                    crate::tcx::GenericParamKind::Const => {
                        // TODO: fresh const inference variables.
                        let ty = self.tcx.interner().mk_ty(Ty::Error);
                        let ct = self
                            .tcx
                            .interner()
                            .mk_const_from_parts(yelang_ty::ty::Const::Error, ty);
                        args.push(yelang_ty::generic::GenericArg::Const(ct));
                    }
                }
            }
        }
        Substitution::from_args(args)
    }

    // -----------------------------------------------------------------------
    // Obligation tracking
    // -----------------------------------------------------------------------

    /// Record a predicate that must hold for this function body to be valid.
    pub fn emit_obligation(&mut self, pred: Predicate) {
        self.obligations.push(pred);
    }

    /// Emit a trait obligation `ty: trait`.
    pub fn emit_trait_obligation(&mut self, ty: TyId, trait_def_id: DefId) {
        let args = self
            .tcx
            .interner()
            .mk_generic_args(&[yelang_ty::generic::GenericArg::Type(ty)]);
        self.emit_obligation(Predicate::Trait(TraitPredicate {
            trait_ref: yelang_ty::predicate::TraitRef {
                def_id: trait_def_id,
                args,
            },
            polarity: ImplPolarity::Positive,
        }));
    }

    /// Emit a well-formedness obligation for a type.
    pub fn emit_wf_obligation(&mut self, ty: TyId) {
        self.emit_obligation(Predicate::WellFormed(
            yelang_ty::predicate::WellFormedPredicate { ty },
        ));
    }

    /// Resolve inference variables inside a predicate as far as possible using
    /// the body `InferCtxt`.
    fn resolve_predicate(&mut self, pred: Predicate) -> Predicate {
        use yelang_ty::fold::{TypeFoldable, TypeFolder, TypeSuperFoldable};

        struct Resolver<'a, 'b> {
            fcx: &'a mut FnCtxt<'b>,
        }

        impl<'a, 'b> TypeFolder for Resolver<'a, 'b> {
            fn interner(&self) -> &Interner {
                self.fcx.tcx.interner()
            }

            fn fold_ty(&mut self, ty: TyId) -> TyId {
                let ty = self.fcx.resolve_ty(ty);
                ty.super_fold_with(self)
            }

            fn fold_const(&mut self, ct: ConstId) -> ConstId {
                let kind = self.interner().const_kind(ct);
                match kind {
                    Const::Infer(vid) => {
                        let root = self.fcx.infer.find_const_var(vid);
                        let probe_result = self.fcx.infer.probe_const_var(root).clone();
                        match probe_result {
                            yelang_infer::ConstVarValue::Known(known) => {
                                let ty = self.interner().const_ty(ct).fold_with(self);
                                let kind = self.interner().const_kind(known);
                                self.interner().mk_const_from_parts(kind, ty)
                            }
                            yelang_infer::ConstVarValue::Unknown => {
                                let ty = self.interner().const_ty(ct).fold_with(self);
                                self.interner().mk_const_from_parts(kind, ty)
                            }
                        }
                    }
                    _ => {
                        let ty = self.interner().const_ty(ct).fold_with(self);
                        self.interner().mk_const_from_parts(kind, ty)
                    }
                }
            }
        }

        pred.fold_with(&mut Resolver { fcx: self })
    }
}

/// A body inference variable that may be constrained by a solver response.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BodyInferVar {
    Ty(TyVid),
    Int(IntVid),
    Float(FloatVid),
    Const(ConstVid),
}

/// Collect the body inference variables in `pred` in first-occurrence order,
/// matching the order produced by the trait solver's canonicalizer.
pub(crate) fn collect_body_infer_vars(
    interner: &Interner,
    infcx: &mut yelang_infer::InferCtxt,
    pred: &Predicate,
) -> Vec<BodyInferVar> {
    struct Collector<'a> {
        interner: &'a Interner,
        infcx: &'a mut yelang_infer::InferCtxt,
        vars: Vec<BodyInferVar>,
        seen_ty: yelang_arena::FxHashSet<TyVid>,
        seen_int: yelang_arena::FxHashSet<IntVid>,
        seen_float: yelang_arena::FxHashSet<FloatVid>,
        seen_const: yelang_arena::FxHashSet<ConstVid>,
    }

    impl<'a> TypeFolder for Collector<'a> {
        fn interner(&self) -> &Interner {
            self.interner
        }

        fn fold_ty(&mut self, ty: TyId) -> TyId {
            match self.interner.ty(ty) {
                Ty::Infer(InferTy::TyVar(vid)) => {
                    let root = self.infcx.find_ty_var(vid);
                    if self.seen_ty.insert(root) {
                        self.vars.push(BodyInferVar::Ty(root));
                    }
                    ty
                }
                Ty::Infer(InferTy::IntVar(vid)) => {
                    let root = self.infcx.find_int_var(vid);
                    if self.seen_int.insert(root) {
                        self.vars.push(BodyInferVar::Int(root));
                    }
                    ty
                }
                Ty::Infer(InferTy::FloatVar(vid)) => {
                    let root = self.infcx.find_float_var(vid);
                    if self.seen_float.insert(root) {
                        self.vars.push(BodyInferVar::Float(root));
                    }
                    ty
                }
                _ => ty.super_fold_with(self),
            }
        }

        fn fold_const(&mut self, ct: ConstId) -> ConstId {
            let kind = self.interner.const_kind(ct);
            match kind {
                Const::Infer(vid) => {
                    let root = self.infcx.find_const_var(vid);
                    if self.seen_const.insert(root) {
                        self.vars.push(BodyInferVar::Const(root));
                    }
                    ct
                }
                _ => ct.super_fold_with(self),
            }
        }
    }

    let mut collector = Collector {
        interner,
        infcx,
        vars: Vec::new(),
        seen_ty: yelang_arena::FxHashSet::default(),
        seen_int: yelang_arena::FxHashSet::default(),
        seen_float: yelang_arena::FxHashSet::default(),
        seen_const: yelang_arena::FxHashSet::default(),
    };
    let _ = (*pred).fold_with(&mut collector);
    collector.vars
}

impl<'a> FnCtxt<'a> {
    /// Prove all accumulated obligations using the next-gen trait solver.
    ///
    /// Returns the list of obligations that could not be proven with certainty.
    /// `NoSolution` results and ambiguous (`Maybe`) goals are both returned as
    /// unproven; Phase E will turn them into diagnostics.
    pub fn prove_obligations(&mut self) -> Vec<UnprovenObligation> {
        let mut unproven = Vec::new();
        let obligations = std::mem::take(&mut self.obligations);

        for pred in obligations {
            let pred = self.resolve_predicate(pred);
            let body_vars = collect_body_infer_vars(self.tcx.interner(), &mut self.infer, &pred);

            let mut ecx = EvalCtxt::new(self.tcx.interner(), self.tcx);
            let goal = Goal::new(self.param_env, pred);
            let canonical_goal = yelang_trait_solver::canonicalize::canonicalize(
                goal,
                self.tcx.interner(),
                &mut self.infer,
                ecx.max_universe(),
            );

            match ecx.evaluate_canonical_goal(canonical_goal) {
                Ok(response) if response.value.certainty == Certainty::Yes => {
                    self.apply_response_to_body(&body_vars, &response);
                }
                Ok(response) => {
                    unproven.push(UnprovenObligation {
                        pred: goal.predicate,
                        status: if response.value.certainty == Certainty::Maybe {
                            ObligationStatus::Ambiguous
                        } else {
                            ObligationStatus::NoSolution
                        },
                    });
                }
                Err(_) => {
                    unproven.push(UnprovenObligation {
                        pred: goal.predicate,
                        status: ObligationStatus::NoSolution,
                    });
                }
            }
        }

        unproven
    }

    /// Apply the inferred values from a solver response back to the body
    /// `InferCtxt`.
    pub(crate) fn apply_response_to_body(
        &mut self,
        body_vars: &[BodyInferVar],
        response: &CanonicalResponse,
    ) {
        let interner = self.tcx.interner();
        for (body_var, value) in body_vars.iter().zip(response.value.var_values.iter()) {
            match (body_var, value) {
                (BodyInferVar::Ty(vid), CanonicalVarValue::Ty(ty)) => {
                    let root = self.infer.find_ty_var(*vid);
                    let _ = self.infer.eq(
                        interner,
                        interner.mk_ty(Ty::Infer(InferTy::TyVar(root))),
                        *ty,
                    );
                }
                (BodyInferVar::Int(vid), CanonicalVarValue::Int(it)) => {
                    let root = self.infer.find_int_var(*vid);
                    let _ = self.infer.set_int_var(root, *it);
                }
                (BodyInferVar::Float(vid), CanonicalVarValue::Float(ft)) => {
                    let root = self.infer.find_float_var(*vid);
                    let _ = self.infer.set_float_var(root, *ft);
                }
                (BodyInferVar::Const(vid), CanonicalVarValue::Const(ct)) => {
                    let root = self.infer.find_const_var(*vid);
                    let _ = self.infer.set_const_var(interner, root, *ct);
                }
                _ => {}
            }
        }
    }
}

impl<'a> FnCtxt<'a> {
    // -----------------------------------------------------------------------
    // Local variable scope management
    // -----------------------------------------------------------------------

    pub fn push_scope(&mut self) {
        self.local_scopes.push(FxHashMap::new());
    }

    pub fn pop_scope(&mut self) {
        self.local_scopes.pop();
    }

    pub fn insert_local(&mut self, pat_id: PatId, ty: TyId) {
        self.local_scopes
            .last_mut()
            .expect("local scope stack should not be empty")
            .insert(pat_id, ty);
        self.results.local_types.insert(pat_id, ty);
    }

    pub fn lookup_local(&self, pat_id: PatId) -> Option<TyId> {
        for scope in self.local_scopes.iter().rev() {
            if let Some(&ty) = scope.get(&pat_id) {
                return Some(ty);
            }
        }
        None
    }

    // -----------------------------------------------------------------------
    // Breakable scope management
    // -----------------------------------------------------------------------

    pub fn push_breakable(&mut self, scope: BreakableScope) {
        self.breakable_scopes.push(scope);
    }

    pub fn pop_breakable(&mut self) -> Option<BreakableScope> {
        self.breakable_scopes.pop()
    }

    pub fn find_breakable(&self, label: Option<&Label>) -> Option<&BreakableScope> {
        if let Some(lbl) = label {
            self.breakable_scopes.iter().rev().find(|s| {
                s.label.as_ref().map(|l| l.symbol.as_usize()) == Some(lbl.symbol.as_usize())
            })
        } else {
            self.breakable_scopes
                .iter()
                .rev()
                .find(|s| s.kind == BreakableKind::Loop)
        }
    }

    // -----------------------------------------------------------------------
    // Item type lookup
    // -----------------------------------------------------------------------

    pub fn item_ty(&self, def_id: DefId) -> Option<TyId> {
        self.tcx.item_ty(def_id)
    }

    // -----------------------------------------------------------------------
    // Type recording
    // -----------------------------------------------------------------------

    pub fn record_expr_ty(&mut self, expr_id: ExprId, ty: TyId) {
        self.results.expr_types.insert(expr_id, ty);
    }

    pub fn record_pat_ty(&mut self, pat_id: PatId, ty: TyId) {
        self.results.pat_types.insert(pat_id, ty);
    }

    // -----------------------------------------------------------------------
    // Type resolution
    // -----------------------------------------------------------------------

    pub fn resolve_ty(&mut self, ty: TyId) -> TyId {
        let interner = self.tcx.interner();
        match interner.ty(ty) {
            Ty::Infer(InferTy::TyVar(vid)) => {
                let root = self.infer.find_ty_var(vid);
                match self.infer.probe_ty_var(root).clone() {
                    TypeVarValue::Known(t) => self.resolve_ty(t),
                    TypeVarValue::Unknown => ty,
                }
            }
            Ty::Infer(InferTy::IntVar(vid)) => {
                let root = self.infer.find_int_var(vid);
                match self.infer.probe_int_var(root).clone() {
                    IntVarValue::Known(it) => self.mk_int(it),
                    IntVarValue::Unknown => ty,
                }
            }
            Ty::Infer(InferTy::FloatVar(vid)) => {
                let root = self.infer.find_float_var(vid);
                match self.infer.probe_float_var(root).clone() {
                    FloatVarValue::Known(ft) => self.mk_float(ft),
                    FloatVarValue::Unknown => ty,
                }
            }
            _ => ty,
        }
    }
}

impl<'a> TyLowerCtxt for FnCtxt<'a> {
    fn interner(&self) -> &Interner {
        self.tcx.interner()
    }

    fn crate_hir(&self) -> &HirCrate {
        self.tcx.crate_hir()
    }

    fn item_ty(&self, def_id: DefId) -> Option<TyId> {
        self.tcx.item_ty(def_id)
    }

    fn self_ty(&self) -> Option<TyId> {
        self.self_ty
    }

    fn lower_infer(&mut self) -> TyId {
        self.new_ty_var()
    }

    fn lower_missing(&mut self) -> TyId {
        self.new_ty_var()
    }

    fn lower_typeof(&mut self, expr: yelang_hir::ids::ExprId) -> TyId {
        crate::check::check_expr(self, expr)
    }
}

// -----------------------------------------------------------------------------
// User-facing formatting for types and obligations.
// -----------------------------------------------------------------------------

/// Format a `TypeError` into a human-readable message.
pub fn format_type_error(tcx: &TyCtxt, err: &TypeError) -> String {
    match err {
        TypeError::Mismatch { expected, found } => format!(
            "type mismatch: expected `{}`, found `{}`",
            format_ty(tcx, *expected),
            format_ty(tcx, *found)
        ),
        TypeError::CyclicTy(vid) => format!("cyclic type: `?T{}`", vid.0),
        TypeError::UnresolvedInferenceVariable(vid) => {
            format!("unresolved inference variable: `?T{}`", vid.0)
        }
        TypeError::ProjectionNotFound(p) => format_projection_predicate(tcx, p),
        TypeError::TraitNotImplemented(p) => format_trait_predicate(tcx, p),
        TypeError::AmbiguousTrait(p) => format!(
            "ambiguous trait bound: `{}`",
            format_trait_predicate(tcx, p)
        ),
        TypeError::NoSuchField { ty, field } => format!(
            "no field `{}` on type `{}`",
            symbol_str(tcx, *field),
            format_ty(tcx, *ty)
        ),
        TypeError::NoSuchMethod { ty, method } => format!(
            "no method `{}` on type `{}`",
            symbol_str(tcx, *method),
            format_ty(tcx, *ty)
        ),
        TypeError::ArgCount { expected, found } => format!(
            "argument count mismatch: expected {}, found {}",
            expected, found
        ),
        TypeError::GenericArgCount { expected, found } => format!(
            "generic argument count mismatch: expected {}, found {}",
            expected, found
        ),
        TypeError::GenericArgKindMismatch { index } => {
            format!("generic argument kind mismatch at index {}", index)
        }
        TypeError::IntMismatch { expected, found } => format!(
            "integer type mismatch: expected `{:?}`, found `{:?}`",
            expected, found
        ),
        TypeError::FloatMismatch { expected, found } => format!(
            "floating-point type mismatch: expected `{:?}`, found `{:?}`",
            expected, found
        ),
        TypeError::ConstMismatch { expected, found } => format!(
            "const mismatch: expected `{:?}`, found `{:?}`",
            expected, found
        ),
        TypeError::TraitRefMismatch { expected, found } => format!(
            "trait reference mismatch: expected `{:?}`, found `{:?}`",
            expected, found
        ),
        TypeError::ExistentialMismatch { expected, found } => format!(
            "existential predicate mismatch: expected `{:?}`, found `{:?}`",
            expected, found
        ),
        TypeError::Custom(msg) => msg.clone(),
    }
}

/// Format an unproven obligation into a human-readable message.
pub fn format_unproven_obligation(tcx: &TyCtxt, obligation: &UnprovenObligation) -> String {
    let pred_str = format_predicate(tcx, &obligation.pred);
    match obligation.status {
        ObligationStatus::Ambiguous => format!("ambiguous trait bound: `{}`", pred_str),
        ObligationStatus::NoSolution => format!("trait bound not satisfied: `{}`", pred_str),
    }
}

/// Format a predicate using item names from `TyCtxt` where possible.
fn format_predicate(tcx: &TyCtxt, pred: &Predicate) -> String {
    match pred {
        Predicate::Trait(tp) => format_trait_predicate(tcx, tp),
        Predicate::Projection(pp) => format_projection_predicate(tcx, pp),
        Predicate::NormalizesTo(np) => format!(
            "<{} as _>::{} = {}",
            format_ty(
                tcx,
                np.projection_ty
                    .trait_ref
                    .args
                    .iter()
                    .next()
                    .map(|a| a.expect_type())
                    .unwrap_or_else(|| tcx.interner().mk_ty(Ty::Error))
            ),
            "Target",
            format_ty(tcx, np.term)
        ),
        Predicate::WellFormed(WellFormedPredicate { ty }) => {
            format!("{} well-formed", format_ty(tcx, *ty))
        }
        Predicate::TypeOutlives(_) => "_: _".to_string(),
        Predicate::ConstEvaluatable(_) => "_: _".to_string(),
    }
}

fn format_trait_predicate(tcx: &TyCtxt, tp: &TraitPredicate) -> String {
    let self_ty = tp
        .trait_ref
        .args
        .iter()
        .next()
        .map(|a| a.expect_type())
        .unwrap_or_else(|| tcx.interner().mk_ty(Ty::Error));
    let trait_name = tcx
        .trait_def(tp.trait_ref.def_id)
        .map(|d| symbol_str(tcx, d.ident.symbol).to_string())
        .unwrap_or_else(|| format!("Trait({})", tp.trait_ref.def_id.raw()));
    let args: Vec<String> = tp
        .trait_ref
        .args
        .iter()
        .skip(1)
        .map(|a| match a {
            GenericArg::Type(ty) => format_ty(tcx, *ty),
            GenericArg::Const(ct) => format!("const#{}?", ct.raw()),
        })
        .collect();
    if args.is_empty() {
        format!("{}: {}", format_ty(tcx, self_ty), trait_name)
    } else {
        format!(
            "{}: {}<{}>",
            format_ty(tcx, self_ty),
            trait_name,
            args.join(", ")
        )
    }
}

fn format_projection_predicate(tcx: &TyCtxt, pp: &ProjectionPredicate) -> String {
    format!(
        "<{} as {}>::{} = {}",
        format_ty(
            tcx,
            pp.projection_ty
                .trait_ref
                .args
                .iter()
                .next()
                .map(|a| a.expect_type())
                .unwrap_or_else(|| tcx.interner().mk_ty(Ty::Error))
        ),
        tcx.trait_def(pp.projection_ty.trait_ref.def_id)
            .map(|d| symbol_str(tcx, d.ident.symbol).to_string())
            .unwrap_or_else(|| "_".to_string()),
        pp.projection_ty.item_def_id.raw(),
        format_ty(tcx, pp.term)
    )
}

/// Format a `TyId` using item names from `TyCtxt` where possible.
pub fn format_ty(tcx: &TyCtxt, ty: TyId) -> String {
    let interner = tcx.interner();
    match interner.ty(ty) {
        Ty::Bool => "bool".to_string(),
        Ty::Char => "char".to_string(),
        Ty::Str => "str".to_string(),
        Ty::Int(it) => match it {
            IntTy::I8 => "i8",
            IntTy::I16 => "i16",
            IntTy::I32 => "i32",
            IntTy::I64 => "i64",
            IntTy::I128 => "i128",
            IntTy::Isize => "isize",
        }
        .to_string(),
        Ty::Uint(it) => match it {
            yelang_ty::primitive::UintTy::U8 => "u8",
            yelang_ty::primitive::UintTy::U16 => "u16",
            yelang_ty::primitive::UintTy::U32 => "u32",
            yelang_ty::primitive::UintTy::U64 => "u64",
            yelang_ty::primitive::UintTy::U128 => "u128",
            yelang_ty::primitive::UintTy::Usize => "usize",
        }
        .to_string(),
        Ty::Float(ft) => match ft {
            yelang_ty::primitive::FloatTy::F32 => "f32",
            yelang_ty::primitive::FloatTy::F64 => "f64",
        }
        .to_string(),
        Ty::Never => "!".to_string(),
        Ty::Param(p) => symbol_str(tcx, p.name).to_string(),
        Ty::Infer(InferTy::TyVar(vid)) => format!("?T{}", vid.0),
        Ty::Infer(InferTy::IntVar(vid)) => format!("?I{}", vid.0),
        Ty::Infer(InferTy::FloatVar(vid)) => format!("?F{}", vid.0),
        Ty::Tuple(args) if args.is_empty() => "()".to_string(),
        Ty::Tuple(args) => {
            let elems: Vec<_> = args.iter().map(|a| format_generic_arg(tcx, a)).collect();
            format!("({})", elems.join(", "))
        }
        Ty::FnPtr(sig) => {
            let inputs: Vec<_> = sig
                .sig
                .inputs
                .iter()
                .map(|a| format_generic_arg(tcx, a))
                .collect();
            format!(
                "fn({}) -> {}",
                inputs.join(", "),
                format_ty(tcx, sig.sig.output)
            )
        }
        Ty::FnDef(def) => format_fn_def(tcx, def.def_id),
        Ty::Adt(adt, args) => {
            let name = tcx
                .adt_def(adt.def_id)
                .map(|d| symbol_str(tcx, d.ident.symbol).to_string())
                .unwrap_or_else(|| format!("Adt({})", adt.def_id.raw()));
            if args.is_empty() {
                name
            } else {
                let targs: Vec<_> = args.iter().map(|a| format_generic_arg(tcx, a)).collect();
                format!("{}<{}>", name, targs.join(", "))
            }
        }
        Ty::Ref(inner, Mutability::Not) => format!("&{}", format_ty(tcx, inner)),
        Ty::Ref(inner, Mutability::Mut) => format!("&mut {}", format_ty(tcx, inner)),
        Ty::RawPtr(TypeAndMut { ty, mutbl }) => {
            let kw = match mutbl {
                Mutability::Mut => "mut",
                Mutability::Not => "const",
            };
            format!("*{} {}", kw, format_ty(tcx, ty))
        }
        Ty::Array(ty, _) => format!("[{}; _]", format_ty(tcx, ty)),
        Ty::Slice(ty) => format!("[{}]", format_ty(tcx, ty)),
        Ty::AnonStruct(anon) => {
            let fields: Vec<_> = anon
                .fields
                .iter()
                .map(|f| format!("{}: {}", symbol_str(tcx, f.name), format_ty(tcx, f.ty)))
                .collect();
            format!("{{ {} }}", fields.join(", "))
        }
        Ty::Error => "{error}".to_string(),
        _ => format!("{:?}", ty),
    }
}

fn format_generic_arg(tcx: &TyCtxt, arg: &GenericArg) -> String {
    match arg {
        GenericArg::Type(ty) => format_ty(tcx, *ty),
        GenericArg::Const(ct) => format!("const#{}?", ct.raw()),
    }
}

fn format_fn_def(tcx: &TyCtxt, def_id: DefId) -> String {
    if let Some(item) = tcx.crate_hir().items.get(def_id).and_then(|o| o.as_ref()) {
        symbol_str(tcx, item.ident.symbol).to_string()
    } else {
        format!("fn_item({})", def_id.raw())
    }
}

fn symbol_str(tcx: &TyCtxt, symbol: yelang_interner::Symbol) -> &str {
    tcx.resolve_symbol(symbol).unwrap_or("_")
}
