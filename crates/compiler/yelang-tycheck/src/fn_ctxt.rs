/*! FnCtxt — the function body type-checking context.
 *
 * Owns the inference state, local variable scopes, and breakable scopes
 * for checking a single function body.
 */

use yelang_arena::{DefId, FxHashMap, index_vec as iv};
use yelang_ast::Label;
use yelang_hir::Crate as HirCrate;
use yelang_hir::ids::{ExprId, PatId};
use yelang_lexer::Span;
use yelang_ty::generic::GenericArg;
use yelang_ty::interner::Interner;
use yelang_ty::list::List;
use yelang_ty::primitive::{FloatTy, IntTy};
use yelang_ty::ty::{AdtDef, Const, InferTy, Mutability, Ty, TyKind, TypeAndMut};

use yelang_infer::context::InferCtxt;
use yelang_infer::error::TypeError;
use yelang_infer::type_variable::{FloatVarValue, IntVarValue, TypeVarValue};
use crate::typeck_results::TypeckResults;

/// A breakable scope for loop/break type checking.
#[derive(Debug, Clone)]
pub struct BreakableScope<'tcx> {
    pub label: Option<Label>,
    pub kind: BreakableKind,
    pub expr_ty: Ty<'tcx>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakableKind {
    Loop,
    Block,
}

/// The function body type-checking context.
pub struct FnCtxt<'tcx> {
    /// The interner for creating canonical types.
    pub interner: &'tcx Interner<'tcx>,
    /// The HIR crate used to look up arena-allocated nodes.
    ///
    /// Immutable: type checking must not mutate the HIR. Results are stored in
    /// `results` and other side tables.
    pub crate_hir: &'tcx HirCrate,
    /// The inference context.
    pub infer: InferCtxt<'tcx>,
    /// Collected results.
    pub results: TypeckResults<'tcx>,
    /// Local variable scope stack. Each frame is a map from PatId to type.
    pub local_scopes: Vec<FxHashMap<PatId, Ty<'tcx>>>,
    /// Breakable scope stack.
    pub breakable_scopes: Vec<BreakableScope<'tcx>>,
    /// The expected return type of the function.
    pub return_ty: Ty<'tcx>,
    /// The self type (if inside an impl).
    pub self_ty: Option<Ty<'tcx>>,
    /// Item types from the collector: DefId -> Ty.
    pub item_types: iv::SecondaryMap<DefId, Ty<'tcx>>,
    /// Whether we're currently in an irrefutable pattern context.
    pub in_irrefutable_pat: bool,
}

impl<'tcx> FnCtxt<'tcx> {
    pub fn new(
        interner: &'tcx Interner<'tcx>,
        crate_hir: &'tcx HirCrate,
        def_id: DefId,
        return_ty: Ty<'tcx>,
        item_types: iv::SecondaryMap<DefId, Ty<'tcx>>,
    ) -> Self {
        Self {
            interner,
            crate_hir,
            infer: InferCtxt::new(),
            results: TypeckResults::new(def_id),
            local_scopes: vec![FxHashMap::new()],
            breakable_scopes: Vec::new(),
            return_ty,
            self_ty: None,
            item_types,
            in_irrefutable_pat: false,
        }
    }

    // -----------------------------------------------------------------------
    // Type creation helpers
    // -----------------------------------------------------------------------

    pub fn mk_ty(&self, kind: TyKind<'tcx>) -> Ty<'tcx> {
        self.interner.mk_ty(kind)
    }

    pub fn mk_tuple(&self, tys: &[Ty<'tcx>]) -> Ty<'tcx> {
        let args = self
            .interner
            .mk_generic_args(&tys.iter().map(|&t| GenericArg::Type(t)).collect::<Vec<_>>());
        self.mk_ty(TyKind::Tuple(args))
    }

    pub fn mk_unit(&self) -> Ty<'tcx> {
        self.mk_ty(TyKind::Tuple(List::empty()))
    }

    pub fn mk_array(&self, ty: Ty<'tcx>, len: Const<'tcx>) -> Ty<'tcx> {
        self.mk_ty(TyKind::Array(ty, len))
    }

    pub fn mk_slice(&self, ty: Ty<'tcx>) -> Ty<'tcx> {
        self.mk_ty(TyKind::Slice(ty))
    }

    pub fn mk_ref(&self, ty: Ty<'tcx>, mutbl: Mutability) -> Ty<'tcx> {
        self.mk_ty(TyKind::Ref(ty, mutbl))
    }

    pub fn mk_raw_ptr(&self, ty: Ty<'tcx>, mutbl: Mutability) -> Ty<'tcx> {
        self.mk_ty(TyKind::RawPtr(TypeAndMut { ty, mutbl }))
    }

    pub fn mk_never(&self) -> Ty<'tcx> {
        self.mk_ty(TyKind::Never)
    }

    pub fn mk_bool(&self) -> Ty<'tcx> {
        self.mk_ty(TyKind::Bool)
    }

    pub fn mk_int(&self, it: IntTy) -> Ty<'tcx> {
        self.mk_ty(TyKind::Int(it))
    }

    pub fn mk_uint(&self, ut: yelang_ty::primitive::UintTy) -> Ty<'tcx> {
        self.mk_ty(TyKind::Uint(ut))
    }

    pub fn mk_float(&self, ft: FloatTy) -> Ty<'tcx> {
        self.mk_ty(TyKind::Float(ft))
    }

    pub fn mk_char(&self) -> Ty<'tcx> {
        self.mk_ty(TyKind::Char)
    }

    pub fn mk_str(&self) -> Ty<'tcx> {
        self.mk_ty(TyKind::Str)
    }

    pub fn mk_error(&self) -> Ty<'tcx> {
        self.mk_ty(TyKind::Error)
    }

    pub fn mk_adt(&self, def_id: DefId, args: List<GenericArg<'tcx>>) -> Ty<'tcx> {
        self.mk_ty(TyKind::Adt(AdtDef { def_id }, args))
    }

    pub fn mk_fn_ptr(&self, inputs: List<GenericArg<'tcx>>, output: Ty<'tcx>) -> Ty<'tcx> {
        self.mk_ty(TyKind::FnPtr(yelang_ty::ty::PolyFnSig {
            sig: yelang_ty::ty::FnSig { inputs, output },
        }))
    }

    // -----------------------------------------------------------------------
    // Inference variable creation
    // -----------------------------------------------------------------------

    pub fn new_ty_var(&mut self) -> Ty<'tcx> {
        self.infer.new_ty_var(self.interner)
    }

    pub fn new_int_var(&mut self) -> Ty<'tcx> {
        self.infer.new_int_var(self.interner)
    }

    pub fn new_float_var(&mut self) -> Ty<'tcx> {
        self.infer.new_float_var(self.interner)
    }

    // -----------------------------------------------------------------------
    // Unification helpers
    // -----------------------------------------------------------------------

    pub fn eq(&mut self, a: Ty<'tcx>, b: Ty<'tcx>) -> Result<(), TypeError<'tcx>> {
        self.infer.eq(a, b)
    }

    pub fn demand_eq(&mut self, span: Span, expected: Ty<'tcx>, found: Ty<'tcx>) -> Ty<'tcx> {
        if let Err(e) = self.eq(expected, found) {
            // TODO: emit diagnostic
            let _ = (span, e);
            self.mk_error()
        } else {
            expected
        }
    }

    // -----------------------------------------------------------------------
    // Local variable scope management
    // -----------------------------------------------------------------------

    pub fn push_scope(&mut self) {
        self.local_scopes.push(FxHashMap::new());
    }

    pub fn pop_scope(&mut self) {
        self.local_scopes.pop();
    }

    pub fn insert_local(&mut self, pat_id: PatId, ty: Ty<'tcx>) {
        self.local_scopes
            .last_mut()
            .expect("local scope stack should not be empty")
            .insert(pat_id, ty);
        self.results.local_types.insert(pat_id, ty);
    }

    pub fn lookup_local(&self, pat_id: PatId) -> Option<Ty<'tcx>> {
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

    pub fn push_breakable(&mut self, scope: BreakableScope<'tcx>) {
        self.breakable_scopes.push(scope);
    }

    pub fn pop_breakable(&mut self) -> Option<BreakableScope<'tcx>> {
        self.breakable_scopes.pop()
    }

    pub fn find_breakable(&self, label: Option<&Label>) -> Option<&BreakableScope<'tcx>> {
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

    pub fn item_ty(&self, def_id: DefId) -> Option<Ty<'tcx>> {
        self.item_types.get(def_id).copied()
    }

    // -----------------------------------------------------------------------
    // Type recording
    // -----------------------------------------------------------------------

    pub fn record_expr_ty(&mut self, expr_id: ExprId, ty: Ty<'tcx>) {
        self.results.expr_types.insert(expr_id, ty);
    }

    pub fn record_pat_ty(&mut self, pat_id: PatId, ty: Ty<'tcx>) {
        self.results.pat_types.insert(pat_id, ty);
    }

    // -----------------------------------------------------------------------
    // Type resolution
    // -----------------------------------------------------------------------

    pub fn resolve_ty(&mut self, ty: Ty<'tcx>) -> Ty<'tcx> {
        match ty.kind() {
            TyKind::Infer(InferTy::TyVar(vid)) => {
                let root = self.infer.find_ty_var(*vid);
                match self.infer.probe_ty_var(root).clone() {
                    TypeVarValue::Known(t) => self.resolve_ty(t),
                    TypeVarValue::Unknown => ty,
                }
            }
            TyKind::Infer(InferTy::IntVar(vid)) => {
                let root = self.infer.find_int_var(*vid);
                match self.infer.probe_int_var(root).clone() {
                    IntVarValue::Known(it) => self.mk_int(it),
                    IntVarValue::Unknown => ty,
                }
            }
            TyKind::Infer(InferTy::FloatVar(vid)) => {
                let root = self.infer.find_float_var(*vid);
                match self.infer.probe_float_var(root).clone() {
                    FloatVarValue::Known(ft) => self.mk_float(ft),
                    FloatVarValue::Unknown => ty,
                }
            }
            _ => ty,
        }
    }
}
