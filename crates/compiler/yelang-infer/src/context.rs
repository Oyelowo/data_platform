/*! Inference context — the main interface for type inference.
 *
 * `InferCtxt` owns the unification tables and provides `eq()` for
 * structural type unification with occurs check.
 */

use yelang_ty::existential::ExistentialPredicate;
use yelang_ty::generic::GenericArg;
use yelang_ty::interner::Interner;
use yelang_ty::list::List;
use yelang_ty::primitive::{FloatTy, IntegerTy, IntTy, UintTy};
use yelang_ty::ty::{
    AnonStructDef, Const, ConstId, ConstVid, FloatVid, FnSig, InferTy, IntVid, Ty, TyId, TyVid,
};

use crate::const_variable::ConstVarValue;
use crate::error::TypeError;
use crate::occurs_check::occurs_check;
use crate::snapshot::Snapshot;
use crate::type_variable::{FloatVarValue, IntVarValue, TypeVarValue, VariableTables};

/// The inference context.
pub struct InferCtxt {
    tables: VariableTables,
}

impl InferCtxt {
    pub fn new() -> Self {
        Self {
            tables: VariableTables::new(),
        }
    }

    // -----------------------------------------------------------------------
    // Variable creation
    // -----------------------------------------------------------------------

    /// Create a new general type variable.
    pub fn new_ty_var(&mut self, interner: &Interner) -> TyId {
        let vid = self.tables.ty_vars.new_var(TypeVarValue::Unknown);
        interner.mk_ty(Ty::Infer(InferTy::TyVar(vid)))
    }

    /// Create a new integral type variable.
    pub fn new_int_var(&mut self, interner: &Interner) -> TyId {
        let vid = self.tables.int_vars.new_var(IntVarValue::Unknown);
        interner.mk_ty(Ty::Infer(InferTy::IntVar(vid)))
    }

    /// Create a new floating-point type variable.
    pub fn new_float_var(&mut self, interner: &Interner) -> TyId {
        let vid = self.tables.float_vars.new_var(FloatVarValue::Unknown);
        interner.mk_ty(Ty::Infer(InferTy::FloatVar(vid)))
    }

    /// Create a new const inference variable with the given expected type.
    pub fn new_const_var(&mut self, interner: &Interner, ty: TyId) -> yelang_ty::ty::ConstId {
        let vid = self.tables.const_vars.new_var(ConstVarValue::Unknown);
        interner.mk_const_from_parts(Const::Infer(vid), ty)
    }

    // -----------------------------------------------------------------------
    // Variable resolution
    // -----------------------------------------------------------------------

    /// Find the root of a general type variable.
    pub fn find_ty_var(&mut self, vid: TyVid) -> TyVid {
        self.tables.ty_vars.find(vid)
    }

    /// Probe the value of a general type variable (with path compression).
    pub fn probe_ty_var(&mut self, vid: TyVid) -> &TypeVarValue {
        self.tables.ty_vars.probe_value(vid)
    }

    /// Find the root of an integral type variable.
    pub fn find_int_var(&mut self, vid: IntVid) -> IntVid {
        self.tables.int_vars.find(vid)
    }

    /// Probe the value of an integral type variable.
    pub fn probe_int_var(&mut self, vid: IntVid) -> &IntVarValue {
        self.tables.int_vars.probe_value(vid)
    }

    /// Find the root of a floating-point type variable.
    pub fn find_float_var(&mut self, vid: FloatVid) -> FloatVid {
        self.tables.float_vars.find(vid)
    }

    /// Probe the value of a floating-point type variable.
    pub fn probe_float_var(&mut self, vid: FloatVid) -> &FloatVarValue {
        self.tables.float_vars.probe_value(vid)
    }

    /// Find the root of a const inference variable.
    pub fn find_const_var(&mut self, vid: ConstVid) -> ConstVid {
        self.tables.const_vars.find(vid)
    }

    /// Probe the value of a const inference variable.
    pub fn probe_const_var(&mut self, vid: ConstVid) -> &ConstVarValue {
        self.tables.const_vars.probe_value(vid)
    }

    /// Set an integral inference variable to a concrete signed integer type.
    pub fn set_int_var(&mut self, vid: IntVid, it: IntTy) -> Result<(), TypeError> {
        self.unify_int_var_value(vid, IntegerTy::Signed(it))
    }

    /// Set an integral inference variable to a concrete unsigned integer type.
    pub fn set_uint_var(&mut self, vid: IntVid, ut: UintTy) -> Result<(), TypeError> {
        self.unify_int_var_value(vid, IntegerTy::Unsigned(ut))
    }

    /// Set a floating-point inference variable to a concrete float type.
    pub fn set_float_var(&mut self, vid: FloatVid, ft: FloatTy) -> Result<(), TypeError> {
        self.unify_float_var_value(vid, ft)
    }

    /// Set a const inference variable to a concrete const.
    pub fn set_const_var(
        &mut self,
        interner: &Interner,
        vid: ConstVid,
        ct: ConstId,
    ) -> Result<(), TypeError> {
        self.unify_const_var_value(interner, vid, ct)
    }

    // -----------------------------------------------------------------------
    // Snapshots
    // -----------------------------------------------------------------------

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            ty: self.tables.ty_vars.snapshot(),
            int: self.tables.int_vars.snapshot(),
            float: self.tables.float_vars.snapshot(),
            const_: self.tables.const_vars.snapshot(),
        }
    }

    pub fn rollback_to(&mut self, snapshot: Snapshot) {
        self.tables.ty_vars.rollback_to(snapshot.ty);
        self.tables.int_vars.rollback_to(snapshot.int);
        self.tables.float_vars.rollback_to(snapshot.float);
        self.tables.const_vars.rollback_to(snapshot.const_);
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
    pub fn eq(&mut self, interner: &Interner, a: TyId, b: TyId) -> Result<(), TypeError> {
        if a == b {
            return Ok(());
        }

        match (interner.ty(a), interner.ty(b)) {
            // Inference variables
            (Ty::Infer(InferTy::TyVar(vid_a)), Ty::Infer(InferTy::TyVar(vid_b))) => {
                self.unify_var_var(interner, vid_a, vid_b)
            }

            // Cross-kind unification: a general type variable with an integer
            // variable. This must come before the generic TyVar/_other arms so
            // that we resolve the integer variable to a concrete integer type
            // rather than storing an IntVar TyId inside the type variable.
            (Ty::Infer(InferTy::TyVar(tv)), Ty::Infer(InferTy::IntVar(iv)))
            | (Ty::Infer(InferTy::IntVar(iv)), Ty::Infer(InferTy::TyVar(tv))) => {
                self.unify_tyvar_with_intvar(interner, tv, iv)
            }

            (Ty::Infer(InferTy::TyVar(vid)), _other) => self.unify_var_value(interner, vid, b),
            (_other, Ty::Infer(InferTy::TyVar(vid))) => self.unify_var_value(interner, vid, a),

            // Int variables (may resolve to signed or unsigned concrete types)
            (Ty::Infer(InferTy::IntVar(vid_a)), Ty::Infer(InferTy::IntVar(vid_b))) => {
                self.tables.int_vars.union(vid_a, vid_b);
                Ok(())
            }

            (Ty::Infer(InferTy::IntVar(vid)), Ty::Int(it)) => {
                self.unify_int_var_value(vid, IntegerTy::Signed(it))
            }
            (Ty::Int(it), Ty::Infer(InferTy::IntVar(vid))) => {
                self.unify_int_var_value(vid, IntegerTy::Signed(it))
            }
            (Ty::Infer(InferTy::IntVar(vid)), Ty::Uint(ut)) => {
                self.unify_int_var_value(vid, IntegerTy::Unsigned(ut))
            }
            (Ty::Uint(ut), Ty::Infer(InferTy::IntVar(vid))) => {
                self.unify_int_var_value(vid, IntegerTy::Unsigned(ut))
            }

            // Float variables
            (Ty::Infer(InferTy::FloatVar(vid_a)), Ty::Infer(InferTy::FloatVar(vid_b))) => {
                self.tables.float_vars.union(vid_a, vid_b);
                Ok(())
            }
            (Ty::Infer(InferTy::FloatVar(vid)), Ty::Float(ft)) => {
                self.unify_float_var_value(vid, ft)
            }
            (Ty::Float(ft), Ty::Infer(InferTy::FloatVar(vid))) => {
                self.unify_float_var_value(vid, ft)
            }

            // Primitive types (already handled by a == b above for interned types)
            (Ty::Bool, Ty::Bool) | (Ty::Char, Ty::Char) | (Ty::Never, Ty::Never) => Ok(()),

            (Ty::Int(a), Ty::Int(b)) if a == b => Ok(()),
            (Ty::Uint(a), Ty::Uint(b)) if a == b => Ok(()),
            (Ty::Float(a), Ty::Float(b)) if a == b => Ok(()),

            // ADT
            (Ty::Adt(def_a, args_a), Ty::Adt(def_b, args_b)) => {
                if def_a.def_id != def_b.def_id {
                    return Err(TypeError::Mismatch {
                        expected: a,
                        found: b,
                    });
                }
                self.eq_generic_args(interner, &args_a, &args_b)
            }

            // Tuples
            (Ty::Tuple(args_a), Ty::Tuple(args_b)) => {
                self.eq_generic_args(interner, &args_a, &args_b)
            }

            // Arrays
            (Ty::Array(ty_a, len_a), Ty::Array(ty_b, len_b)) => {
                self.eq(interner, ty_a, ty_b)?;
                self.eq_const(interner, len_a, len_b)
            }

            // Slices
            (Ty::Slice(ty_a), Ty::Slice(ty_b)) => self.eq(interner, ty_a, ty_b),

            // Raw pointers
            (Ty::RawPtr(tam_a), Ty::RawPtr(tam_b)) => {
                if tam_a.mutbl != tam_b.mutbl {
                    return Err(TypeError::Mismatch {
                        expected: a,
                        found: b,
                    });
                }
                self.eq(interner, tam_a.ty, tam_b.ty)
            }

            // References
            (Ty::Ref(ty_a, mut_a), Ty::Ref(ty_b, mut_b)) => {
                if mut_a != mut_b {
                    return Err(TypeError::Mismatch {
                        expected: a,
                        found: b,
                    });
                }
                self.eq(interner, ty_a, ty_b)
            }

            // Functions
            (Ty::FnPtr(sig_a), Ty::FnPtr(sig_b)) => {
                self.eq_fn_sigs(interner, &sig_a.sig, &sig_b.sig)
            }
            (Ty::FnDef(fd_a), Ty::FnDef(fd_b)) => {
                if fd_a.def_id != fd_b.def_id {
                    return Err(TypeError::Mismatch {
                        expected: a,
                        found: b,
                    });
                }
                self.eq_generic_args(interner, &fd_a.args, &fd_b.args)
            }

            // Anonymous structs
            (Ty::AnonStruct(anon_a), Ty::AnonStruct(anon_b)) => {
                self.eq_anon_structs(interner, &anon_a, &anon_b, a, b)
            }

            // Unions
            (Ty::Union(a1, a2), Ty::Union(b1, b2)) => {
                self.eq(interner, a1, b1)?;
                self.eq(interner, a2, b2)
            }

            // Type literals
            (Ty::TypeLit(sym_a), Ty::TypeLit(sym_b)) => {
                if sym_a != sym_b {
                    return Err(TypeError::Mismatch {
                        expected: a,
                        found: b,
                    });
                }
                Ok(())
            }

            // Utility types
            (Ty::Utility(kind_a, args_a), Ty::Utility(kind_b, args_b)) => {
                if kind_a != kind_b {
                    return Err(TypeError::Mismatch {
                        expected: a,
                        found: b,
                    });
                }
                self.eq_generic_args(interner, &args_a, &args_b)
            }

            // Aliases (associated types / impl Trait)
            (Ty::Alias(alias_a), Ty::Alias(alias_b)) => {
                if alias_a.def_id != alias_b.def_id {
                    return Err(TypeError::Mismatch {
                        expected: a,
                        found: b,
                    });
                }
                self.eq_generic_args(interner, &alias_a.args, &alias_b.args)
            }

            // Projection types
            (Ty::Projection(proj_a), Ty::Projection(proj_b)) => {
                if proj_a.item_def_id != proj_b.item_def_id {
                    return Err(TypeError::Mismatch {
                        expected: a,
                        found: b,
                    });
                }
                self.eq_trait_refs(interner, &proj_a.trait_ref, &proj_b.trait_ref)
            }

            // Trait objects
            (Ty::Dynamic(binder_a), Ty::Dynamic(binder_b)) => {
                if binder_a.bound_vars != binder_b.bound_vars {
                    return Err(TypeError::Mismatch {
                        expected: a,
                        found: b,
                    });
                }
                self.eq_dynamic_predicates(interner, a, b, binder_a.value, binder_b.value)
            }

            // Placeholders
            (Ty::Placeholder(p_a), Ty::Placeholder(p_b)) => {
                if p_a != p_b {
                    return Err(TypeError::Mismatch {
                        expected: a,
                        found: b,
                    });
                }
                Ok(())
            }

            // Bound variables (under the same binder, same index)
            (Ty::Bound(d_a, b_a), Ty::Bound(d_b, b_b)) if d_a == d_b && b_a == b_b => Ok(()),

            // Error type unifies with anything (error recovery)
            (Ty::Error, _) | (_, Ty::Error) => Ok(()),

            // Mismatch
            _ => Err(TypeError::Mismatch {
                expected: a,
                found: b,
            }),
        }
    }

    // -----------------------------------------------------------------------
    // Helper unification methods
    // -----------------------------------------------------------------------

    fn unify_var_var(
        &mut self,
        interner: &Interner,
        vid_a: TyVid,
        vid_b: TyVid,
    ) -> Result<(), TypeError> {
        let val_a = self
            .tables
            .ty_vars
            .probe_value_no_compression(vid_a)
            .clone();
        let val_b = self
            .tables
            .ty_vars
            .probe_value_no_compression(vid_b)
            .clone();
        self.tables.ty_vars.union(vid_a, vid_b);

        match (val_a, val_b) {
            (TypeVarValue::Known(ty_a), TypeVarValue::Known(ty_b)) => {
                self.eq(interner, ty_a, ty_b)?;
            }
            (TypeVarValue::Known(ty), TypeVarValue::Unknown) => {
                self.tables
                    .ty_vars
                    .set_value(vid_a, TypeVarValue::Known(ty));
            }
            (TypeVarValue::Unknown, TypeVarValue::Known(ty)) => {
                self.tables
                    .ty_vars
                    .set_value(vid_b, TypeVarValue::Known(ty));
            }
            (TypeVarValue::Unknown, TypeVarValue::Unknown) => {}
        }

        Ok(())
    }

    fn unify_var_value(
        &mut self,
        interner: &Interner,
        vid: TyVid,
        ty: TyId,
    ) -> Result<(), TypeError> {
        // Occurs check: `vid` must not appear inside `ty`.
        if occurs_check(interner, &mut self.tables, vid, ty) {
            return Err(TypeError::CyclicTy(vid));
        }

        let root = self.tables.ty_vars.find(vid);
        let existing = self.tables.ty_vars.probe_value(root).clone();

        match existing {
            TypeVarValue::Known(existing_ty) => {
                self.eq(interner, existing_ty, ty)?;
            }
            TypeVarValue::Unknown => {
                self.tables.ty_vars.set_value(root, TypeVarValue::Known(ty));
            }
        }

        Ok(())
    }

    fn unify_int_var_value(&mut self, vid: IntVid, it: IntegerTy) -> Result<(), TypeError> {
        let root = self.tables.int_vars.find(vid);
        let existing = self.tables.int_vars.probe_value(root).clone();
        match existing {
            IntVarValue::Known(existing_it) => {
                if existing_it != it {
                    return Err(TypeError::IntMismatch {
                        expected: existing_it,
                        found: it,
                    });
                }
            }
            IntVarValue::Unknown => {
                self.tables.int_vars.set_value(root, IntVarValue::Known(it));
            }
        }
        Ok(())
    }

    fn unify_float_var_value(&mut self, vid: FloatVid, ft: FloatTy) -> Result<(), TypeError> {
        let root = self.tables.float_vars.find(vid);
        let existing = self.tables.float_vars.probe_value(root).clone();
        match existing {
            FloatVarValue::Known(existing_ft) => {
                if existing_ft != ft {
                    return Err(TypeError::FloatMismatch {
                        expected: existing_ft,
                        found: ft,
                    });
                }
            }
            FloatVarValue::Unknown => {
                self.tables
                    .float_vars
                    .set_value(root, FloatVarValue::Known(ft));
            }
        }
        Ok(())
    }

    /// Unify a general type variable with an integer inference variable.
    ///
    /// If the type variable is already known to be an integer type, the integer
    /// variable is set to that type. If the integer variable is known, the type
    /// variable is set to that concrete integer type. If both are unknown, the
    /// type variable is bound to the integer variable so that later fallback
    /// (e.g. `i32` defaulting at writeback) can resolve it without prematurely
    /// committing to a concrete type.
    fn unify_tyvar_with_intvar(
        &mut self,
        interner: &Interner,
        tv: TyVid,
        iv: IntVid,
    ) -> Result<(), TypeError> {
        let tv_root = self.tables.ty_vars.find(tv);
        let tv_existing = self.tables.ty_vars.probe_value(tv_root).clone();

        if let TypeVarValue::Known(ty) = tv_existing {
            return match interner.ty(ty) {
                Ty::Int(it) => self.unify_int_var_value(iv, IntegerTy::Signed(it)),
                Ty::Uint(ut) => self.unify_int_var_value(iv, IntegerTy::Unsigned(ut)),
                _ => Err(TypeError::Mismatch {
                    expected: interner.mk_ty(Ty::Infer(InferTy::TyVar(tv_root))),
                    found: interner.mk_ty(Ty::Infer(InferTy::IntVar(iv))),
                }),
            };
        }

        let iv_root = self.tables.int_vars.find(iv);
        let iv_existing = self.tables.int_vars.probe_value(iv_root).clone();

        match iv_existing {
            IntVarValue::Known(it) => {
                let concrete_ty = match it {
                    IntegerTy::Signed(it) => interner.mk_ty(Ty::Int(it)),
                    IntegerTy::Unsigned(ut) => interner.mk_ty(Ty::Uint(ut)),
                };
                self.unify_var_value(interner, tv_root, concrete_ty)
            }
            IntVarValue::Unknown => {
                let int_var_ty = interner.mk_ty(Ty::Infer(InferTy::IntVar(iv_root)));
                self.unify_var_value(interner, tv_root, int_var_ty)
            }
        }
    }

    // -----------------------------------------------------------------------
    // Structural helpers
    // -----------------------------------------------------------------------

    pub fn eq_generic_args(
        &mut self,
        interner: &Interner,
        a: &List<GenericArg>,
        b: &List<GenericArg>,
    ) -> Result<(), TypeError> {
        if a.len() != b.len() {
            return Err(TypeError::GenericArgCount {
                expected: a.len(),
                found: b.len(),
            });
        }
        for (index, (arg_a, arg_b)) in a.iter().zip(b.iter()).enumerate() {
            match (arg_a, arg_b) {
                (GenericArg::Type(ta), GenericArg::Type(tb)) => self.eq(interner, *ta, *tb)?,
                (GenericArg::Const(ca), GenericArg::Const(cb)) => {
                    self.eq_const(interner, *ca, *cb)?
                }
                _ => {
                    return Err(TypeError::GenericArgKindMismatch { index });
                }
            }
        }
        Ok(())
    }

    pub(crate) fn eq_const(
        &mut self,
        interner: &Interner,
        a: yelang_ty::ty::ConstId,
        b: yelang_ty::ty::ConstId,
    ) -> Result<(), TypeError> {
        self.eq(interner, interner.const_ty(a), interner.const_ty(b))?;
        match (interner.const_kind(a), interner.const_kind(b)) {
            (Const::Infer(vid_a), Const::Infer(vid_b)) => {
                self.tables.const_vars.union(vid_a, vid_b);
                Ok(())
            }
            (Const::Infer(vid), _) => self.unify_const_var_value(interner, vid, b),
            (_, Const::Infer(vid)) => self.unify_const_var_value(interner, vid, a),
            (Const::Value(va), Const::Value(vb)) => {
                if va != vb {
                    return Err(TypeError::ConstMismatch {
                        expected: a,
                        found: b,
                    });
                }
                Ok(())
            }
            (Const::Param(pa), Const::Param(pb)) if pa == pb => Ok(()),
            (Const::Placeholder(pa), Const::Placeholder(pb)) if pa == pb => Ok(()),
            (Const::Bound(da, ba), Const::Bound(db, bb)) if da == db && ba == bb => Ok(()),
            (Const::Unevaluated(ua), Const::Unevaluated(ub)) => {
                if ua.def != ub.def {
                    return Err(TypeError::ConstMismatch {
                        expected: a,
                        found: b,
                    });
                }
                self.eq_generic_args(interner, &ua.args, &ub.args)
            }
            (Const::Error, _) | (_, Const::Error) => Ok(()),
            _ => Err(TypeError::ConstMismatch {
                expected: a,
                found: b,
            }),
        }
    }

    fn unify_const_var_value(
        &mut self,
        interner: &Interner,
        vid: ConstVid,
        ct: yelang_ty::ty::ConstId,
    ) -> Result<(), TypeError> {
        let root = self.tables.const_vars.find(vid);
        let existing = self.tables.const_vars.probe_value(root).clone();
        match existing {
            ConstVarValue::Known(existing_ct) => self.eq_const(interner, existing_ct, ct),
            ConstVarValue::Unknown => {
                self.tables
                    .const_vars
                    .set_value(root, ConstVarValue::Known(ct));
                Ok(())
            }
        }
    }

    fn eq_fn_sigs(&mut self, interner: &Interner, a: &FnSig, b: &FnSig) -> Result<(), TypeError> {
        self.eq_generic_args(interner, &a.inputs, &b.inputs)?;
        self.eq(interner, a.output, b.output)
    }

    fn eq_anon_structs(
        &mut self,
        interner: &Interner,
        a: &AnonStructDef,
        b: &AnonStructDef,
        ty_a: TyId,
        ty_b: TyId,
    ) -> Result<(), TypeError> {
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
            self.eq(interner, f_a.ty, f_b.ty)?;
        }
        Ok(())
    }

    pub fn eq_trait_refs(
        &mut self,
        interner: &Interner,
        a: &yelang_ty::predicate::TraitRef,
        b: &yelang_ty::predicate::TraitRef,
    ) -> Result<(), TypeError> {
        if a.def_id != b.def_id {
            return Err(TypeError::TraitRefMismatch {
                expected: *a,
                found: *b,
            });
        }
        self.eq_generic_args(interner, &a.args, &b.args)
    }

    fn eq_dynamic_predicates(
        &mut self,
        interner: &Interner,
        ty_a: TyId,
        ty_b: TyId,
        a: List<ExistentialPredicate>,
        b: List<ExistentialPredicate>,
    ) -> Result<(), TypeError> {
        if a.len() != b.len() {
            return Err(TypeError::Mismatch {
                expected: ty_a,
                found: ty_b,
            });
        }
        for (pa, pb) in a.iter().zip(b.iter()) {
            self.eq_existential_predicates(interner, *pa, *pb)
                .map_err(|_| TypeError::Mismatch {
                    expected: ty_a,
                    found: ty_b,
                })?;
        }
        Ok(())
    }

    fn eq_existential_predicates(
        &mut self,
        interner: &Interner,
        a: ExistentialPredicate,
        b: ExistentialPredicate,
    ) -> Result<(), TypeError> {
        match (a, b) {
            (ExistentialPredicate::Trait(ta), ExistentialPredicate::Trait(tb)) => {
                if ta.def_id != tb.def_id {
                    return Err(TypeError::ExistentialMismatch {
                        expected: a,
                        found: b,
                    });
                }
                self.eq_generic_args(interner, &ta.args, &tb.args)
            }
            (ExistentialPredicate::Projection(pa), ExistentialPredicate::Projection(pb)) => {
                if pa.def_id != pb.def_id {
                    return Err(TypeError::ExistentialMismatch {
                        expected: a,
                        found: b,
                    });
                }
                self.eq_generic_args(interner, &pa.args, &pb.args)?;
                self.eq(interner, pa.term, pb.term)
            }
            (ExistentialPredicate::AutoTrait(da), ExistentialPredicate::AutoTrait(db)) => {
                if da != db {
                    return Err(TypeError::ExistentialMismatch {
                        expected: a,
                        found: b,
                    });
                }
                Ok(())
            }
            _ => Err(TypeError::ExistentialMismatch {
                expected: a,
                found: b,
            }),
        }
    }
}

impl Default for InferCtxt {
    fn default() -> Self {
        Self::new()
    }
}
