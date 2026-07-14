/*! Inference context — the main interface for type inference.
 *
 * `InferCtxt` owns the unification tables and provides `eq()` for
 * structural type unification with occurs check.
 */

use yelang_ty::generic::GenericArg;
use yelang_ty::list::List;
use yelang_ty::primitive::{FloatTy, IntTy};
use yelang_ty::ty::{
    AnonStructDef, Const, ConstKind, ExistentialPredicate, FloatVid, FnSig, InferTy, IntVid, Ty,
    TyKind, TyVid,
};

use crate::error::TypeError;
use crate::snapshot::Snapshot;
use crate::type_variable::{FloatVarValue, IntVarValue, TypeVarValue, VariableTables};
use crate::unify::UnifyKey;

/// The inference context.
pub struct InferCtxt<'tcx> {
    tables: VariableTables<'tcx>,
}

impl<'tcx> InferCtxt<'tcx> {
    pub fn new() -> Self {
        Self {
            tables: VariableTables::new(),
        }
    }

    // -----------------------------------------------------------------------
    // Variable creation
    // -----------------------------------------------------------------------

    /// Create a new general type variable.
    pub fn new_ty_var(&mut self) -> Ty<'tcx> {
        let _vid = self.tables.ty_vars.new_var(TypeVarValue::Unknown);
        // We need an Interner to construct TyKind::Infer. This is a design
        // issue: InferCtxt doesn't currently have an Interner.
        // FIXME: Integrate with Interner or TyCtxt.
        // For now, we can't construct a Ty without an Interner.
        panic!("InferCtxt needs an Interner to construct inference variable types")
    }

    // -----------------------------------------------------------------------
    // Snapshots
    // -----------------------------------------------------------------------

    pub fn snapshot(&self) -> Snapshot {
        self.tables.ty_vars.snapshot()
    }

    pub fn rollback_to(&mut self, snapshot: Snapshot) {
        self.tables.ty_vars.rollback_to(snapshot);
        self.tables.int_vars.rollback_to(snapshot);
        self.tables.float_vars.rollback_to(snapshot);
    }

    /// Execute `f` within a speculative snapshot, rolling back on failure.
    pub fn probe<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let snapshot = self.snapshot();
        let result = f(self);
        self.rollback_to(snapshot);
        result
    }

    // -----------------------------------------------------------------------
    // Core unification: eq
    // -----------------------------------------------------------------------

    /// Unify two types structurally.
    pub fn eq(&mut self, a: Ty<'tcx>, b: Ty<'tcx>) -> Result<(), TypeError<'tcx>> {
        if a == b {
            return Ok(());
        }

        match (a.kind(), b.kind()) {
            // Inference variables
            (TyKind::Infer(InferTy::TyVar(vid_a)), TyKind::Infer(InferTy::TyVar(vid_b))) => {
                self.unify_var_var(*vid_a, *vid_b)
            }
            (TyKind::Infer(InferTy::TyVar(vid)), _other) => {
                self.unify_var_value(*vid, b)
            }
            (_other, TyKind::Infer(InferTy::TyVar(vid))) => {
                self.unify_var_value(*vid, a)
            }

            // Int variables
            (TyKind::Infer(InferTy::IntVar(vid_a)), TyKind::Infer(InferTy::IntVar(vid_b))) => {
                self.tables.int_vars.union(*vid_a, *vid_b).map_err(|_| {
                    TypeError::Custom("int var union failed".into())
                })
            }
            (TyKind::Infer(InferTy::IntVar(vid)), TyKind::Int(it)) => {
                self.unify_int_var_value(*vid, *it)
            }
            (TyKind::Int(it), TyKind::Infer(InferTy::IntVar(vid))) => {
                self.unify_int_var_value(*vid, *it)
            }

            // Float variables
            (TyKind::Infer(InferTy::FloatVar(vid_a)), TyKind::Infer(InferTy::FloatVar(vid_b))) => {
                self.tables.float_vars.union(*vid_a, *vid_b).map_err(|_| {
                    TypeError::Custom("float var union failed".into())
                })
            }
            (TyKind::Infer(InferTy::FloatVar(vid)), TyKind::Float(ft)) => {
                self.unify_float_var_value(*vid, *ft)
            }
            (TyKind::Float(ft), TyKind::Infer(InferTy::FloatVar(vid))) => {
                self.unify_float_var_value(*vid, *ft)
            }

            // Primitive types (already handled by a == b above for interned types)
            (TyKind::Bool, TyKind::Bool)
            | (TyKind::Char, TyKind::Char)
            | (TyKind::Never, TyKind::Never) => Ok(()),

            (TyKind::Int(a), TyKind::Int(b)) if a == b => Ok(()),
            (TyKind::Uint(a), TyKind::Uint(b)) if a == b => Ok(()),
            (TyKind::Float(a), TyKind::Float(b)) if a == b => Ok(()),

            // ADT
            (TyKind::Adt(def_a, args_a), TyKind::Adt(def_b, args_b)) => {
                if def_a.def_id != def_b.def_id {
                    return Err(TypeError::Mismatch { expected: a, found: b });
                }
                self.eq_generic_args(args_a, args_b)
            }

            // Tuples
            (TyKind::Tuple(args_a), TyKind::Tuple(args_b)) => {
                self.eq_generic_args(args_a, args_b)
            }

            // Arrays
            (TyKind::Array(ty_a, len_a), TyKind::Array(ty_b, len_b)) => {
                self.eq(*ty_a, *ty_b)?;
                self.eq_const(*len_a, *len_b)
            }

            // Slices
            (TyKind::Slice(ty_a), TyKind::Slice(ty_b)) => {
                self.eq(*ty_a, *ty_b)
            }

            // Raw pointers
            (TyKind::RawPtr(tam_a), TyKind::RawPtr(tam_b)) => {
                if tam_a.mutbl != tam_b.mutbl {
                    return Err(TypeError::Mismatch { expected: a, found: b });
                }
                self.eq(tam_a.ty, tam_b.ty)
            }

            // References
            (TyKind::Ref(ty_a, mut_a), TyKind::Ref(ty_b, mut_b)) => {
                if mut_a != mut_b {
                    return Err(TypeError::Mismatch { expected: a, found: b });
                }
                self.eq(*ty_a, *ty_b)
            }

            // Functions
            (TyKind::FnPtr(sig_a), TyKind::FnPtr(sig_b)) => {
                self.eq_fn_sigs(&sig_a.sig, &sig_b.sig)
            }
            (TyKind::FnDef(fd_a), TyKind::FnDef(fd_b)) => {
                if fd_a.def_id != fd_b.def_id {
                    return Err(TypeError::Mismatch { expected: a, found: b });
                }
                self.eq_generic_args(&fd_a.args, &fd_b.args)
            }

            // Anonymous structs
            (TyKind::AnonStruct(anon_a), TyKind::AnonStruct(anon_b)) => {
                self.eq_anon_structs(anon_a, anon_b, a, b)
            }

            // Unions
            (TyKind::Union(a1, a2), TyKind::Union(b1, b2)) => {
                self.eq(*a1, *b1)?;
                self.eq(*a2, *b2)
            }

            // Type literals
            (TyKind::TypeLit(sym_a), TyKind::TypeLit(sym_b)) => {
                if sym_a != sym_b {
                    return Err(TypeError::Mismatch { expected: a, found: b });
                }
                Ok(())
            }

            // Utility types
            (TyKind::Utility(kind_a, args_a), TyKind::Utility(kind_b, args_b)) => {
                if kind_a != kind_b {
                    return Err(TypeError::Mismatch { expected: a, found: b });
                }
                self.eq_generic_args(args_a, args_b)
            }

            // Aliases (associated types / impl Trait)
            (TyKind::Alias(alias_a), TyKind::Alias(alias_b)) => {
                if alias_a.def_id != alias_b.def_id {
                    return Err(TypeError::Mismatch { expected: a, found: b });
                }
                self.eq_generic_args(&alias_a.args, &alias_b.args)
            }

            // Trait objects
            (TyKind::Dynamic(binder_a), TyKind::Dynamic(binder_b)) => {
                self.eq_existential_predicates(binder_a.value, binder_b.value)
            }

            // Placeholders
            (TyKind::Placeholder(p_a), TyKind::Placeholder(p_b)) => {
                if p_a != p_b {
                    return Err(TypeError::Mismatch { expected: a, found: b });
                }
                Ok(())
            }

            // Error type unifies with anything (error recovery)
            (TyKind::Error, _) | (_, TyKind::Error) => Ok(()),

            // Mismatch
            _ => Err(TypeError::Mismatch { expected: a, found: b }),
        }
    }

    // -----------------------------------------------------------------------
    // Helper unification methods
    // -----------------------------------------------------------------------

    fn unify_var_var(&mut self, vid_a: TyVid, vid_b: TyVid) -> Result<(), TypeError<'tcx>> {
        let val_a = self.tables.ty_vars.probe_value_no_compression(vid_a).clone();
        let val_b = self.tables.ty_vars.probe_value_no_compression(vid_b).clone();
        self.tables.ty_vars.union(vid_a, vid_b).map_err(|_| {
            TypeError::Custom("var-var union failed".into())
        })?;

        match (&val_a, &val_b) {
            (TypeVarValue::Known(ty_a), TypeVarValue::Known(ty_b)) => {
                self.eq(*ty_a, *ty_b)?;
            }
            (TypeVarValue::Known(ty), TypeVarValue::Unknown) => {
                self.tables.ty_vars.set_value(vid_a, TypeVarValue::Known(*ty));
            }
            (TypeVarValue::Unknown, TypeVarValue::Known(ty)) => {
                self.tables.ty_vars.set_value(vid_b, TypeVarValue::Known(*ty));
            }
            (TypeVarValue::Unknown, TypeVarValue::Unknown) => {}
        }

        Ok(())
    }

    fn unify_var_value(&mut self, vid: TyVid, ty: Ty<'tcx>) -> Result<(), TypeError<'tcx>> {
        // Occurs check: `vid` must not appear inside `ty`.
        if self.occurs_check(vid, ty) {
            return Err(TypeError::CyclicTy(vid));
        }

        let root = self.tables.ty_vars.find(vid);
        let existing = self.tables.ty_vars.probe_value(root).clone();

        match existing {
            TypeVarValue::Known(existing_ty) => {
                self.eq(existing_ty, ty)?;
            }
            TypeVarValue::Unknown => {
                self.tables.ty_vars.set_value(root, TypeVarValue::Known(ty));
            }
        }

        Ok(())
    }

    fn unify_int_var_value(&mut self, vid: IntVid, it: IntTy) -> Result<(), TypeError<'tcx>> {
        let root = self.tables.int_vars.find(vid);
        let existing = self.tables.int_vars.probe_value(root).clone();
        match existing {
            IntVarValue::Known(existing_it) => {
                if existing_it != it {
                    return Err(TypeError::Custom(format!(
                        "int type mismatch: expected {:?}, found {:?}",
                        existing_it, it
                    )));
                }
            }
            IntVarValue::Unknown => {
                self.tables.int_vars.set_value(root, IntVarValue::Known(it));
            }
        }
        Ok(())
    }

    fn unify_float_var_value(&mut self, vid: FloatVid, ft: FloatTy) -> Result<(), TypeError<'tcx>> {
        let root = self.tables.float_vars.find(vid);
        let existing = self.tables.float_vars.probe_value(root).clone();
        match existing {
            FloatVarValue::Known(existing_ft) => {
                if existing_ft != ft {
                    return Err(TypeError::Custom(format!(
                        "float type mismatch: expected {:?}, found {:?}",
                        existing_ft, ft
                    )));
                }
            }
            FloatVarValue::Unknown => {
                self.tables.float_vars.set_value(root, FloatVarValue::Known(ft));
            }
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Occurs check
    // -----------------------------------------------------------------------

    fn occurs_check(&mut self, vid: TyVid, ty: Ty<'tcx>) -> bool {
        match ty.kind() {
            TyKind::Infer(InferTy::TyVar(other_vid)) => {
                // Normalize: find the root of other_vid.
                let root = self.tables.ty_vars.find(*other_vid);
                root.index() == vid.index()
            }
            TyKind::Adt(_, args) => args.iter().any(|arg| match arg {
                GenericArg::Type(t) => self.occurs_check(vid, *t),
                GenericArg::Const(_) => false,
            }),
            TyKind::Tuple(args) => args.iter().any(|arg| match arg {
                GenericArg::Type(t) => self.occurs_check(vid, *t),
                GenericArg::Const(_) => false,
            }),
            TyKind::Array(ty, _) => self.occurs_check(vid, *ty),
            TyKind::Slice(ty) => self.occurs_check(vid, *ty),
            TyKind::RawPtr(tam) => self.occurs_check(vid, tam.ty),
            TyKind::Ref(ty, _) => self.occurs_check(vid, *ty),
            TyKind::FnPtr(sig) => sig
                .sig
                .inputs
                .iter()
                .any(|arg| match arg {
                    GenericArg::Type(t) => self.occurs_check(vid, *t),
                    GenericArg::Const(_) => false,
                })
                || self.occurs_check(vid, sig.sig.output),
            TyKind::FnDef(fd) => fd.args.iter().any(|arg| match arg {
                GenericArg::Type(t) => self.occurs_check(vid, *t),
                GenericArg::Const(_) => false,
            }),
            TyKind::AnonStruct(anon) => anon.fields.iter().any(|f| self.occurs_check(vid, f.ty)),
            TyKind::Union(a, b) => self.occurs_check(vid, *a) || self.occurs_check(vid, *b),
            TyKind::Utility(_, args) => args.iter().any(|arg| match arg {
                GenericArg::Type(t) => self.occurs_check(vid, *t),
                GenericArg::Const(_) => false,
            }),
            TyKind::Alias(alias) => alias.args.iter().any(|arg| match arg {
                GenericArg::Type(t) => self.occurs_check(vid, *t),
                GenericArg::Const(_) => false,
            }),
            TyKind::Dynamic(binder) => match binder.value {
                ExistentialPredicate::Trait(tr) => tr.args.iter().any(|arg| match arg {
                    GenericArg::Type(t) => self.occurs_check(vid, *t),
                    GenericArg::Const(_) => false,
                }),
                ExistentialPredicate::Projection(pr) => {
                    pr.args.iter().any(|arg| match arg {
                        GenericArg::Type(t) => self.occurs_check(vid, *t),
                        GenericArg::Const(_) => false,
                    }) || self.occurs_check(vid, pr.term)
                }
                ExistentialPredicate::AutoTrait(_) => false,
            },
            // All other cases don't contain nested types.
            _ => false,
        }
    }

    // -----------------------------------------------------------------------
    // Structural helpers
    // -----------------------------------------------------------------------

    fn eq_generic_args(
        &mut self,
        a: &List<GenericArg<'tcx>>,
        b: &List<GenericArg<'tcx>>,
    ) -> Result<(), TypeError<'tcx>> {
        if a.len() != b.len() {
            return Err(TypeError::GenericArgCount {
                expected: a.len(),
                found: b.len(),
            });
        }
        for (arg_a, arg_b) in a.iter().zip(b.iter()) {
            match (arg_a, arg_b) {
                (GenericArg::Type(ta), GenericArg::Type(tb)) => self.eq(*ta, *tb)?,
                (GenericArg::Const(ca), GenericArg::Const(cb)) => self.eq_const(*ca, *cb)?,
                _ => {
                    return Err(TypeError::Custom(
                        "generic argument kind mismatch".into(),
                    ))
                }
            }
        }
        Ok(())
    }

    fn eq_const(&mut self, a: Const<'tcx>, b: Const<'tcx>) -> Result<(), TypeError<'tcx>> {
        self.eq(a.ty, b.ty)?;
        match (&a.kind, &b.kind) {
            (ConstKind::Value(va), ConstKind::Value(vb)) => {
                if va != vb {
                    return Err(TypeError::Custom("const value mismatch".into()));
                }
                Ok(())
            }
            (ConstKind::Error, _) | (_, ConstKind::Error) => Ok(()),
            _ => Err(TypeError::Custom("const kind mismatch".into())),
        }
    }

    fn eq_fn_sigs(&mut self, a: &FnSig<'tcx>, b: &FnSig<'tcx>) -> Result<(), TypeError<'tcx>> {
        self.eq_generic_args(&a.inputs, &b.inputs)?;
        self.eq(a.output, b.output)
    }

    fn eq_anon_structs(
        &mut self,
        a: &AnonStructDef<'tcx>,
        b: &AnonStructDef<'tcx>,
        ty_a: Ty<'tcx>,
        ty_b: Ty<'tcx>,
    ) -> Result<(), TypeError<'tcx>> {
        // For anonymous structs, we require exact field match (for now).
        // Width subtyping is handled as coercion, not unification.
        if a.fields.len() != b.fields.len() {
            return Err(TypeError::Mismatch {
                expected: ty_a,
                found: ty_b,
            });
        }
        for (f_a, f_b) in a.fields.iter().zip(b.fields.iter()) {
            if f_a.name != f_b.name {
                return Err(TypeError::Mismatch {
                    expected: ty_a,
                    found: ty_b,
                });
            }
            self.eq(f_a.ty, f_b.ty)?;
        }
        Ok(())
    }

    fn eq_existential_predicates(
        &mut self,
        a: ExistentialPredicate<'tcx>,
        b: ExistentialPredicate<'tcx>,
    ) -> Result<(), TypeError<'tcx>> {
        match (a, b) {
            (ExistentialPredicate::Trait(ta), ExistentialPredicate::Trait(tb)) => {
                if ta.def_id != tb.def_id {
                    return Err(TypeError::Custom("trait object trait mismatch".into()));
                }
                self.eq_generic_args(&ta.args, &tb.args)
            }
            (ExistentialPredicate::Projection(pa), ExistentialPredicate::Projection(pb)) => {
                if pa.def_id != pb.def_id {
                    return Err(TypeError::Custom("trait object projection mismatch".into()));
                }
                self.eq_generic_args(&pa.args, &pb.args)?;
                self.eq(pa.term, pb.term)
            }
            (ExistentialPredicate::AutoTrait(da), ExistentialPredicate::AutoTrait(db)) => {
                if da != db {
                    return Err(TypeError::Custom("auto trait mismatch".into()));
                }
                Ok(())
            }
            _ => Err(TypeError::Custom("existential predicate kind mismatch".into())),
        }
    }
}

impl<'tcx> Default for InferCtxt<'tcx> {
    fn default() -> Self {
        Self::new()
    }
}
