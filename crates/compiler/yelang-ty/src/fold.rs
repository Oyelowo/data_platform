/*! Structural folding over types.
 *
 * `TypeFoldable` allows replacing parts of a type (e.g., substituting
 * parameters). `TypeFolder` defines the replacement logic.
 *
 * Every folder has access to the interner so that folded types and lists can
 * be re-interned, preserving the invariant that structurally equal types share
 * an ID.
 */

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
    AnonField, Const, ConstId, ExistentialProjection, ExistentialTraitRef, GenericArgsRef,
    ProjectionTy, Ty, TyId, TypeAndMut,
};

/// A folder that transforms types.
pub trait TypeFolder: Sized {
    /// The interner used to re-intern folded types and lists.
    fn interner(&self) -> &Interner;

    /// Fold a type. The default delegates to structural folding.
    fn fold_ty(&mut self, ty: TyId) -> TyId {
        ty.super_fold_with(self)
    }

    /// Fold a constant. The default returns the constant unchanged.
    fn fold_const(&mut self, ct: ConstId) -> ConstId {
        ct
    }
}

/// Types that can be structurally folded.
pub trait TypeFoldable: Sized {
    fn fold_with<F: TypeFolder>(self, folder: &mut F) -> Self;
}

/// Super-fold: the default structural traversal for a type.
pub trait TypeSuperFoldable: TypeFoldable {
    fn super_fold_with<F: TypeFolder>(self, folder: &mut F) -> Self;
}

// ---------------------------------------------------------------------------
// TyId
// ---------------------------------------------------------------------------

impl TypeFoldable for TyId {
    fn fold_with<F: TypeFolder>(self, folder: &mut F) -> Self {
        folder.fold_ty(self)
    }
}

impl TypeSuperFoldable for TyId {
    fn super_fold_with<F: TypeFolder>(self, folder: &mut F) -> Self {
        let kind = folder.interner().ty(self);
        let kind = match kind {
            Ty::Bool => Ty::Bool,
            Ty::Char => Ty::Char,
            Ty::Str => Ty::Str,
            Ty::Int(it) => Ty::Int(it),
            Ty::Uint(ut) => Ty::Uint(ut),
            Ty::Float(ft) => Ty::Float(ft),
            Ty::Param(p) => Ty::Param(p),
            Ty::Bound(d, bt) => Ty::Bound(d, bt),
            Ty::Infer(iv) => Ty::Infer(iv),
            Ty::Adt(def, args) => Ty::Adt(def, args.fold_with(folder)),
            Ty::FnPtr(sig) => Ty::FnPtr(crate::ty::PolyFnSig {
                sig: crate::ty::FnSig {
                    inputs: sig.sig.inputs.fold_with(folder),
                    output: sig.sig.output.fold_with(folder),
                    return_ty_infer: sig.sig.return_ty_infer,
                },
            }),
            Ty::FnDef(fd) => Ty::FnDef(crate::ty::FnDef {
                def_id: fd.def_id,
                args: fd.args.fold_with(folder),
            }),
            Ty::Tuple(args) => Ty::Tuple(args.fold_with(folder)),
            Ty::Array(ty, ct) => Ty::Array(ty.fold_with(folder), ct.fold_with(folder)),
            Ty::Slice(ty) => Ty::Slice(ty.fold_with(folder)),
            Ty::RawPtr(tam) => Ty::RawPtr(TypeAndMut {
                ty: tam.ty.fold_with(folder),
                mutbl: tam.mutbl,
            }),
            Ty::Ref(ty, mutbl) => Ty::Ref(ty.fold_with(folder), mutbl),
            Ty::Never => Ty::Never,
            Ty::AnonStruct(anon) => {
                let fields: Vec<_> = anon
                    .fields
                    .iter()
                    .map(|f| AnonField {
                        name: f.name,
                        ty: f.ty.fold_with(folder),
                    })
                    .collect();
                Ty::AnonStruct(crate::ty::AnonStructDef {
                    fields: folder.interner().mk_anon_struct_fields(&fields),
                })
            }
            Ty::Union(a, b) => Ty::Union(a.fold_with(folder), b.fold_with(folder)),
            Ty::TypeLit(sym) => Ty::TypeLit(sym),
            Ty::Utility(k, args) => Ty::Utility(k, args.fold_with(folder)),
            Ty::Alias(alias) => Ty::Alias(crate::ty::AliasTy {
                def_id: alias.def_id,
                args: alias.args.fold_with(folder),
            }),
            Ty::Projection(proj) => Ty::Projection(ProjectionTy {
                trait_ref: proj.trait_ref.fold_with(folder),
                item_def_id: proj.item_def_id,
            }),
            Ty::Dynamic(binder) => Ty::Dynamic(crate::ty::Binder {
                bound_vars: binder.bound_vars.fold_with(folder),
                value: binder.value.fold_with(folder),
            }),
            Ty::Placeholder(p) => Ty::Placeholder(p),
            Ty::Error => Ty::Error,
        };
        folder.interner().mk_ty(kind)
    }
}

// ---------------------------------------------------------------------------
// Lists
// ---------------------------------------------------------------------------

impl TypeFoldable for GenericArgsRef {
    fn fold_with<F: TypeFolder>(self, folder: &mut F) -> Self {
        let folded: Vec<_> = self.iter().map(|&arg| arg.fold_with(folder)).collect();
        folder.interner().mk_generic_args(&folded)
    }
}

impl TypeFoldable for List<BoundVariableKind> {
    fn fold_with<F: TypeFolder>(self, folder: &mut F) -> Self {
        let folded: Vec<_> = self.iter().copied().collect();
        // Bound variable kinds do not contain nested types/consts, so no recursion.
        folder.interner().mk_bound_var_list(&folded)
    }
}

impl TypeFoldable for List<ExistentialPredicate> {
    fn fold_with<F: TypeFolder>(self, folder: &mut F) -> Self {
        let folded: Vec<_> = self.iter().map(|&p| p.fold_with(folder)).collect();
        folder.interner().mk_existential_predicates(&folded)
    }
}

impl TypeFoldable for List<TyId> {
    fn fold_with<F: TypeFolder>(self, folder: &mut F) -> Self {
        let folded: Vec<_> = self.iter().map(|&ty| ty.fold_with(folder)).collect();
        folder.interner().mk_ty_list(&folded)
    }
}

// ---------------------------------------------------------------------------
// GenericArg
// ---------------------------------------------------------------------------

impl TypeFoldable for GenericArg {
    fn fold_with<F: TypeFolder>(self, folder: &mut F) -> Self {
        match self {
            GenericArg::Type(ty) => GenericArg::Type(ty.fold_with(folder)),
            GenericArg::Const(ct) => GenericArg::Const(ct.fold_with(folder)),
        }
    }
}

// ---------------------------------------------------------------------------
// ConstId
// ---------------------------------------------------------------------------

impl TypeFoldable for ConstId {
    fn fold_with<F: TypeFolder>(self, folder: &mut F) -> Self {
        folder.fold_const(self)
    }
}

impl TypeSuperFoldable for ConstId {
    fn super_fold_with<F: TypeFolder>(self, folder: &mut F) -> Self {
        let interner = folder.interner();
        let kind = interner.const_kind(self);
        let ty = interner.const_ty(self).fold_with(folder);
        let kind = match kind {
            Const::Value(v) => Const::Value(v),
            Const::Param(p) => Const::Param(p),
            Const::Bound(d, bv) => Const::Bound(d, bv),
            Const::Placeholder(p) => Const::Placeholder(p),
            Const::Unevaluated(u) => Const::Unevaluated(crate::ty::UnevaluatedConst {
                def: u.def,
                args: u.args.fold_with(folder),
            }),
            Const::Infer(v) => Const::Infer(v),
            Const::Error => Const::Error,
        };
        folder.interner().mk_const_from_parts(kind, ty)
    }
}

// ---------------------------------------------------------------------------
// Binder
// ---------------------------------------------------------------------------

impl<T: TypeFoldable + Copy> TypeFoldable for crate::ty::Binder<T> {
    fn fold_with<F: TypeFolder>(self, folder: &mut F) -> Self {
        crate::ty::Binder {
            bound_vars: self.bound_vars.fold_with(folder),
            value: self.value.fold_with(folder),
        }
    }
}

// ---------------------------------------------------------------------------
// Predicate pieces
// ---------------------------------------------------------------------------

impl TypeFoldable for TraitRef {
    fn fold_with<F: TypeFolder>(self, folder: &mut F) -> Self {
        TraitRef {
            def_id: self.def_id,
            args: self.args.fold_with(folder),
        }
    }
}

impl TypeFoldable for ProjectionTy {
    fn fold_with<F: TypeFolder>(self, folder: &mut F) -> Self {
        ProjectionTy {
            trait_ref: self.trait_ref.fold_with(folder),
            item_def_id: self.item_def_id,
        }
    }
}

impl TypeFoldable for ExistentialPredicate {
    fn fold_with<F: TypeFolder>(self, folder: &mut F) -> Self {
        match self {
            ExistentialPredicate::Trait(tr) => ExistentialPredicate::Trait(ExistentialTraitRef {
                def_id: tr.def_id,
                args: tr.args.fold_with(folder),
            }),
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

impl TypeFoldable for Predicate {
    fn fold_with<F: TypeFolder>(self, folder: &mut F) -> Self {
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
            Predicate::ConstEvaluatable(ct) => Predicate::ConstEvaluatable(ct.fold_with(folder)),
        }
    }
}

impl TypeFoldable for List<Predicate> {
    fn fold_with<F: TypeFolder>(self, folder: &mut F) -> Self {
        let folded: Vec<_> = self.iter().map(|&p| p.fold_with(folder)).collect();
        folder.interner().mk_predicates(&folded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interner::Interner;
    use crate::primitive::IntTy;
    use crate::ty::{AdtDef, ParamTy, Ty};
    use yelang_interner::Symbol;

    struct IdentityFolder<'a> {
        interner: &'a Interner,
    }

    impl<'a> TypeFolder for IdentityFolder<'a> {
        fn interner(&self) -> &Interner {
            self.interner
        }
    }

    #[test]
    fn fold_identity_preserves_interning() {
        let interner = Interner::new();
        let mut folder = IdentityFolder {
            interner: &interner,
        };

        let t_i32 = interner.mk_ty(Ty::Int(IntTy::I32));
        let folded = t_i32.fold_with(&mut folder);
        assert_eq!(t_i32, folded);
    }

    struct ReplaceParamFolder<'a> {
        interner: &'a Interner,
        replacement: TyId,
    }

    impl<'a> TypeFolder for ReplaceParamFolder<'a> {
        fn interner(&self) -> &Interner {
            self.interner
        }

        fn fold_ty(&mut self, ty: TyId) -> TyId {
            if matches!(self.interner.ty(ty), Ty::Param(_)) {
                self.replacement
            } else {
                ty.super_fold_with(self)
            }
        }
    }

    #[test]
    fn fold_substitutes_param() {
        let interner = Interner::new();
        let t_param = interner.mk_ty(Ty::Param(ParamTy {
            index: 0,
            name: Symbol::from(1),
        }));
        let t_i32 = interner.mk_ty(Ty::Int(IntTy::I32));
        let t_tuple = interner.mk_ty(Ty::Tuple(
            interner.mk_generic_args(&[GenericArg::Type(t_param)]),
        ));

        let mut folder = ReplaceParamFolder {
            interner: &interner,
            replacement: t_i32,
        };
        let folded = t_tuple.fold_with(&mut folder);

        match interner.ty(folded) {
            Ty::Tuple(args) => {
                assert_eq!(args.len(), 1);
                assert_eq!(args[0].expect_type(), t_i32);
            }
            _ => panic!("expected tuple"),
        }
    }

    #[test]
    fn fold_adt_args() {
        let interner = Interner::new();
        let t_param = interner.mk_ty(Ty::Param(ParamTy {
            index: 0,
            name: Symbol::from(1),
        }));
        let t_bool = interner.mk_ty(Ty::Bool);
        let args = interner.mk_generic_args(&[GenericArg::Type(t_param), GenericArg::Type(t_bool)]);
        let t_adt = interner.mk_ty(Ty::Adt(
            AdtDef {
                def_id: yelang_arena::DefId::new(1),
            },
            args,
        ));

        let mut folder = ReplaceParamFolder {
            interner: &interner,
            replacement: interner.mk_ty(Ty::Int(IntTy::I64)),
        };
        let folded = t_adt.fold_with(&mut folder);

        match interner.ty(folded) {
            Ty::Adt(_, args) => {
                assert_eq!(args[0].expect_type(), interner.mk_ty(Ty::Int(IntTy::I64)));
                assert_eq!(args[1].expect_type(), t_bool);
            }
            _ => panic!("expected adt"),
        }
    }
}
