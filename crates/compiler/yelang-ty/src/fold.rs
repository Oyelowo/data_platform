/*! Structural folding over types.
 *
 * `TypeFoldable` allows replacing parts of a type (e.g., substituting
 * parameters). `TypeFolder` defines the replacement logic.
 *
 * Every folder has access to the interner so that folded types and lists can
 * be re-interned, preserving the invariant that structurally equal types share
 * an allocation.
 */

use std::marker::PhantomData;

use crate::binder::BoundVariableKind;
use crate::existential::ExistentialPredicate;
use crate::generic::GenericArg;
use crate::interner::Interner;
use crate::list::List;
use crate::predicate::{
    NormalizesToPredicate, Predicate, ProjectionPredicate, TraitPredicate, TraitRef,
    TypeOutlivesPredicate, WellFormedPredicate,
};
use crate::ty::{
    AnonField, Const, ConstKind, ExistentialProjection, ExistentialTraitRef, GenericArgsRef,
    ProjectionTy, Ty, TyKind, TypeAndMut,
};

/// A folder that transforms types.
pub trait TypeFolder<'tcx>: Sized {
    /// The interner used to re-intern folded types and lists.
    fn interner(&self) -> &'tcx Interner<'tcx>;

    /// Fold a type. The default delegates to structural folding.
    fn fold_ty(&mut self, ty: Ty<'tcx>) -> Ty<'tcx> {
        ty.super_fold_with(self)
    }

    /// Fold a constant. The default returns the constant unchanged.
    fn fold_const(&mut self, ct: Const<'tcx>) -> Const<'tcx> {
        ct
    }
}

/// Types that can be structurally folded.
pub trait TypeFoldable<'tcx>: Sized {
    fn fold_with<F: TypeFolder<'tcx>>(self, folder: &mut F) -> Self;
}

/// Super-fold: the default structural traversal for a type.
pub trait TypeSuperFoldable<'tcx>: TypeFoldable<'tcx> {
    fn super_fold_with<F: TypeFolder<'tcx>>(self, folder: &mut F) -> Self;
}

// ---------------------------------------------------------------------------
// Ty
// ---------------------------------------------------------------------------

impl<'tcx> TypeFoldable<'tcx> for Ty<'tcx> {
    fn fold_with<F: TypeFolder<'tcx>>(self, folder: &mut F) -> Self {
        folder.fold_ty(self)
    }
}

impl<'tcx> TypeSuperFoldable<'tcx> for Ty<'tcx> {
    fn super_fold_with<F: TypeFolder<'tcx>>(self, folder: &mut F) -> Self {
        let interner = folder.interner();
        let kind = match *self.kind() {
            TyKind::Bool => TyKind::Bool,
            TyKind::Char => TyKind::Char,
            TyKind::Str => TyKind::Str,
            TyKind::Int(it) => TyKind::Int(it),
            TyKind::Uint(ut) => TyKind::Uint(ut),
            TyKind::Float(ft) => TyKind::Float(ft),
            TyKind::Param(p) => TyKind::Param(p),
            TyKind::Bound(d, bt) => TyKind::Bound(d, bt),
            TyKind::Infer(iv) => TyKind::Infer(iv),
            TyKind::Adt(def, args) => TyKind::Adt(def, args.fold_with(folder)),
            TyKind::FnPtr(sig) => TyKind::FnPtr(crate::ty::PolyFnSig {
                sig: crate::ty::FnSig {
                    inputs: sig.sig.inputs.fold_with(folder),
                    output: sig.sig.output.fold_with(folder),
                },
            }),
            TyKind::FnDef(fd) => TyKind::FnDef(crate::ty::FnDef {
                def_id: fd.def_id,
                args: fd.args.fold_with(folder),
            }),
            TyKind::Tuple(args) => TyKind::Tuple(args.fold_with(folder)),
            TyKind::Array(ty, ct) => TyKind::Array(ty.fold_with(folder), ct.fold_with(folder)),
            TyKind::Slice(ty) => TyKind::Slice(ty.fold_with(folder)),
            TyKind::RawPtr(tam) => TyKind::RawPtr(TypeAndMut {
                ty: tam.ty.fold_with(folder),
                mutbl: tam.mutbl,
            }),
            TyKind::Ref(ty, mutbl) => TyKind::Ref(ty.fold_with(folder), mutbl),
            TyKind::Never => TyKind::Never,
            TyKind::AnonStruct(anon) => {
                let fields: Vec<_> = anon
                    .fields
                    .iter()
                    .map(|f| AnonField {
                        name: f.name,
                        ty: f.ty.fold_with(folder),
                    })
                    .collect();
                TyKind::AnonStruct(crate::ty::AnonStructDef {
                    fields: interner.mk_list(&fields),
                })
            }
            TyKind::Union(a, b) => TyKind::Union(a.fold_with(folder), b.fold_with(folder)),
            TyKind::TypeLit(sym) => TyKind::TypeLit(sym),
            TyKind::Utility(k, args) => TyKind::Utility(k, args.fold_with(folder)),
            TyKind::Alias(alias) => TyKind::Alias(crate::ty::AliasTy {
                def_id: alias.def_id,
                args: alias.args.fold_with(folder),
            }),
            TyKind::Projection(proj) => TyKind::Projection(ProjectionTy {
                trait_ref: proj.trait_ref.fold_with(folder),
                item_def_id: proj.item_def_id,
            }),
            TyKind::Dynamic(binder) => TyKind::Dynamic(crate::ty::Binder {
                bound_vars: binder.bound_vars.fold_with(folder),
                value: binder.value.fold_with(folder),
                _marker: PhantomData,
            }),
            TyKind::Placeholder(p) => TyKind::Placeholder(p),
            TyKind::Error => TyKind::Error,
        };
        interner.mk_ty(kind)
    }
}

// ---------------------------------------------------------------------------
// Lists
// ---------------------------------------------------------------------------

impl<'tcx> TypeFoldable<'tcx> for GenericArgsRef<'tcx> {
    fn fold_with<F: TypeFolder<'tcx>>(self, folder: &mut F) -> Self {
        let interner = folder.interner();
        let folded: Vec<_> = self.iter().map(|&arg| arg.fold_with(folder)).collect();
        interner.mk_generic_args(&folded)
    }
}

impl<'tcx> TypeFoldable<'tcx> for List<BoundVariableKind> {
    fn fold_with<F: TypeFolder<'tcx>>(self, folder: &mut F) -> Self {
        let interner = folder.interner();
        let folded: Vec<_> = self.iter().copied().collect();
        // Bound variable kinds do not contain nested types/consts, so no recursion.
        interner.mk_bound_var_list(&folded)
    }
}

impl<'tcx> TypeFoldable<'tcx> for List<ExistentialPredicate<'tcx>> {
    fn fold_with<F: TypeFolder<'tcx>>(self, folder: &mut F) -> Self {
        let interner = folder.interner();
        let folded: Vec<_> = self.iter().map(|&p| p.fold_with(folder)).collect();
        interner.mk_existential_predicates(&folded)
    }
}

impl<'tcx> TypeFoldable<'tcx> for List<Ty<'tcx>> {
    fn fold_with<F: TypeFolder<'tcx>>(self, folder: &mut F) -> Self {
        let interner = folder.interner();
        let folded: Vec<_> = self.iter().map(|&ty| ty.fold_with(folder)).collect();
        interner.mk_ty_list(&folded)
    }
}

// ---------------------------------------------------------------------------
// GenericArg
// ---------------------------------------------------------------------------

impl<'tcx> TypeFoldable<'tcx> for GenericArg<'tcx> {
    fn fold_with<F: TypeFolder<'tcx>>(self, folder: &mut F) -> Self {
        match self {
            GenericArg::Type(ty) => GenericArg::Type(ty.fold_with(folder)),
            GenericArg::Const(ct) => GenericArg::Const(ct.fold_with(folder)),
        }
    }
}

// ---------------------------------------------------------------------------
// Const
// ---------------------------------------------------------------------------

impl<'tcx> TypeFoldable<'tcx> for Const<'tcx> {
    fn fold_with<F: TypeFolder<'tcx>>(self, folder: &mut F) -> Self {
        folder.fold_const(self)
    }
}

impl<'tcx> TypeSuperFoldable<'tcx> for Const<'tcx> {
    fn super_fold_with<F: TypeFolder<'tcx>>(self, folder: &mut F) -> Self {
        let ty = self.ty.fold_with(folder);
        let kind = match self.kind {
            ConstKind::Value(v) => ConstKind::Value(v),
            ConstKind::Param(p) => ConstKind::Param(p),
            ConstKind::Bound(d, bv) => ConstKind::Bound(d, bv),
            ConstKind::Placeholder(p) => ConstKind::Placeholder(p),
            ConstKind::Unevaluated(u) => ConstKind::Unevaluated(crate::ty::UnevaluatedConst {
                def: u.def,
                args: u.args.fold_with(folder),
            }),
            ConstKind::Infer(v) => ConstKind::Infer(v),
            ConstKind::Error => ConstKind::Error,
        };
        Const { kind, ty }
    }
}

// ---------------------------------------------------------------------------
// Binder
// ---------------------------------------------------------------------------

impl<'tcx, T: TypeFoldable<'tcx> + Copy + 'tcx> TypeFoldable<'tcx> for crate::ty::Binder<'tcx, T> {
    fn fold_with<F: TypeFolder<'tcx>>(self, folder: &mut F) -> Self {
        crate::ty::Binder {
            bound_vars: self.bound_vars.fold_with(folder),
            value: self.value.fold_with(folder),
            _marker: PhantomData,
        }
    }
}

// ---------------------------------------------------------------------------
// Predicate pieces
// ---------------------------------------------------------------------------

impl<'tcx> TypeFoldable<'tcx> for TraitRef<'tcx> {
    fn fold_with<F: TypeFolder<'tcx>>(self, folder: &mut F) -> Self {
        TraitRef {
            def_id: self.def_id,
            args: self.args.fold_with(folder),
        }
    }
}

impl<'tcx> TypeFoldable<'tcx> for ProjectionTy<'tcx> {
    fn fold_with<F: TypeFolder<'tcx>>(self, folder: &mut F) -> Self {
        ProjectionTy {
            trait_ref: self.trait_ref.fold_with(folder),
            item_def_id: self.item_def_id,
        }
    }
}

impl<'tcx> TypeFoldable<'tcx> for ExistentialPredicate<'tcx> {
    fn fold_with<F: TypeFolder<'tcx>>(self, folder: &mut F) -> Self {
        match self {
            ExistentialPredicate::Trait(tr) => {
                ExistentialPredicate::Trait(ExistentialTraitRef {
                    def_id: tr.def_id,
                    args: tr.args.fold_with(folder),
                })
            }
            ExistentialPredicate::Projection(pr) => {
                ExistentialPredicate::Projection(ExistentialProjection {
                    def_id: pr.def_id,
                    args: pr.args.fold_with(folder),
                    term: pr.term.fold_with(folder),
                })
            }
            ExistentialPredicate::AutoTrait(d) => ExistentialPredicate::AutoTrait(d),
        }
    }
}

impl<'tcx> TypeFoldable<'tcx> for Predicate<'tcx> {
    fn fold_with<F: TypeFolder<'tcx>>(self, folder: &mut F) -> Self {
        match self {
            Predicate::Trait(p) => Predicate::Trait(TraitPredicate {
                trait_ref: p.trait_ref.fold_with(folder),
                polarity: p.polarity,
            }),
            Predicate::Projection(p) => Predicate::Projection(ProjectionPredicate {
                projection_ty: p.projection_ty.fold_with(folder),
                term: p.term.fold_with(folder),
            }),
            Predicate::NormalizesTo(p) => Predicate::NormalizesTo(NormalizesToPredicate {
                projection_ty: p.projection_ty.fold_with(folder),
                term: p.term.fold_with(folder),
            }),
            Predicate::WellFormed(p) => Predicate::WellFormed(WellFormedPredicate {
                ty: p.ty.fold_with(folder),
            }),
            Predicate::TypeOutlives(p) => Predicate::TypeOutlives(TypeOutlivesPredicate {
                ty: p.ty.fold_with(folder),
            }),
            Predicate::ConstEvaluatable(ct) => {
                Predicate::ConstEvaluatable(ct.fold_with(folder))
            }
        }
    }
}

impl<'tcx> TypeFoldable<'tcx> for List<Predicate<'tcx>> {
    fn fold_with<F: TypeFolder<'tcx>>(self, folder: &mut F) -> Self {
        let interner = folder.interner();
        let folded: Vec<_> = self.iter().map(|&p| p.fold_with(folder)).collect();
        interner.mk_predicates(&folded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interner::Interner;
    use crate::primitive::IntTy;
    use crate::ty::{AdtDef, ParamTy, TyKind};
    use yelang_interner::Symbol;

    struct IdentityFolder<'tcx> {
        interner: &'tcx Interner<'tcx>,
    }

    impl<'tcx> TypeFolder<'tcx> for IdentityFolder<'tcx> {
        fn interner(&self) -> &'tcx Interner<'tcx> {
            self.interner
        }
    }

    #[test]
    fn fold_identity_preserves_interning() {
        let interner = Interner::new();
        let folder = IdentityFolder { interner: &interner };

        let t_i32 = interner.mk_ty(TyKind::Int(IntTy::I32));
        let folded = t_i32.fold_with(&mut { folder });
        assert_eq!(t_i32, folded);
    }

    struct ReplaceParamFolder<'tcx> {
        interner: &'tcx Interner<'tcx>,
        replacement: Ty<'tcx>,
    }

    impl<'tcx> TypeFolder<'tcx> for ReplaceParamFolder<'tcx> {
        fn interner(&self) -> &'tcx Interner<'tcx> {
            self.interner
        }

        fn fold_ty(&mut self, ty: Ty<'tcx>) -> Ty<'tcx> {
            if let TyKind::Param(_) = ty.kind() {
                self.replacement
            } else {
                ty.super_fold_with(self)
            }
        }
    }

    #[test]
    fn fold_substitutes_param() {
        let interner = Interner::new();
        let t_param = interner.mk_ty(TyKind::Param(ParamTy {
            index: 0,
            name: Symbol::from(1),
        }));
        let t_i32 = interner.mk_ty(TyKind::Int(IntTy::I32));
        let t_tuple = interner.mk_ty(TyKind::Tuple(
            interner.mk_generic_args(&[GenericArg::Type(t_param)]),
        ));

        let mut folder = ReplaceParamFolder {
            interner: &interner,
            replacement: t_i32,
        };
        let folded = t_tuple.fold_with(&mut folder);

        match folded.kind() {
            TyKind::Tuple(args) => {
                assert_eq!(args.len(), 1);
                assert_eq!(args[0].expect_type(), t_i32);
            }
            _ => panic!("expected tuple"),
        }
    }

    #[test]
    fn fold_adt_args() {
        let interner = Interner::new();
        let t_param = interner.mk_ty(TyKind::Param(ParamTy {
            index: 0,
            name: Symbol::from(1),
        }));
        let t_bool = interner.mk_ty(TyKind::Bool);
        let args = interner.mk_generic_args(&[
            GenericArg::Type(t_param),
            GenericArg::Type(t_bool),
        ]);
        let t_adt = interner.mk_ty(TyKind::Adt(AdtDef { def_id: yelang_arena::DefId::new(1) }, args));

        let mut folder = ReplaceParamFolder {
            interner: &interner,
            replacement: interner.mk_ty(TyKind::Int(IntTy::I64)),
        };
        let folded = t_adt.fold_with(&mut folder);

        match folded.kind() {
            TyKind::Adt(_, args) => {
                assert_eq!(args[0].expect_type(), interner.mk_ty(TyKind::Int(IntTy::I64)));
                assert_eq!(args[1].expect_type(), t_bool);
            }
            _ => panic!("expected adt"),
        }
    }
}
