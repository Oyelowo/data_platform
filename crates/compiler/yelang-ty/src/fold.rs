/*! Structural folding over types.
 *
 * `TypeFoldable` allows replacing parts of a type (e.g., substituting
 * parameters). `TypeFolder` defines the replacement logic.
 */

use crate::list::List;
use crate::ty::{Const, Ty, TyKind};

/// A folder that transforms types.
pub trait TypeFolder<'tcx>: Sized {
    fn fold_ty(&mut self, ty: Ty<'tcx>) -> Ty<'tcx> {
        ty.super_fold_with(self)
    }

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
// Implementations
// ---------------------------------------------------------------------------

impl<'tcx> TypeFoldable<'tcx> for Ty<'tcx> {
    fn fold_with<F: TypeFolder<'tcx>>(self, folder: &mut F) -> Self {
        folder.fold_ty(self)
    }
}

impl<'tcx> TypeSuperFoldable<'tcx> for Ty<'tcx> {
    fn super_fold_with<F: TypeFolder<'tcx>>(self, folder: &mut F) -> Self {
        let _kind = match *self.kind() {
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
            TyKind::Array(ty, ct) => TyKind::Array(ty.fold_with(folder), ct),
            TyKind::Slice(ty) => TyKind::Slice(ty.fold_with(folder)),
            TyKind::RawPtr(tam) => TyKind::RawPtr(crate::ty::TypeAndMut {
                ty: tam.ty.fold_with(folder),
                mutbl: tam.mutbl,
            }),
            TyKind::Ref(ty, mutbl) => TyKind::Ref(ty.fold_with(folder), mutbl),
            TyKind::Never => TyKind::Never,
            TyKind::AnonStruct(anon) => {
                let fields: Vec<_> = anon
                    .fields
                    .iter()
                    .map(|f| crate::ty::AnonField {
                        name: f.name,
                        ty: f.ty.fold_with(folder),
                    })
                    .collect();
                // Note: requires Interner to re-intern; for now this is a sketch.
                TyKind::AnonStruct(crate::ty::AnonStructDef {
                    fields: List::from_slice(&fields),
                })
            }
            TyKind::Union(a, b) => TyKind::Union(a.fold_with(folder), b.fold_with(folder)),
            TyKind::TypeLit(sym) => TyKind::TypeLit(sym),
            TyKind::Utility(k, args) => TyKind::Utility(k, args.fold_with(folder)),
            TyKind::Alias(alias) => TyKind::Alias(crate::ty::AliasTy {
                def_id: alias.def_id,
                args: alias.args.fold_with(folder),
            }),
            TyKind::Dynamic(binder) => TyKind::Dynamic(crate::ty::Binder {
                bound_vars: binder.bound_vars,
                value: binder.value,
                _marker: std::marker::PhantomData,
            }),
            TyKind::Placeholder(p) => TyKind::Placeholder(p),
            TyKind::Error => TyKind::Error,
        };
        // In a real implementation, we would use Interner::mk_ty here.
        // For the fold trait, we return a TyKind; the caller must intern.
        // This is a design limitation we'll resolve when integrating.
        // For now, we just return the same Ty since we can't intern without context.
        // FIXME: properly integrate with Interner.
        self
    }
}

impl<'tcx, T: TypeFoldable<'tcx> + Copy> TypeFoldable<'tcx> for List<T> {
    fn fold_with<F: TypeFolder<'tcx>>(self, _folder: &mut F) -> Self {
        // For now, return unchanged. A real implementation would re-intern.
        self
    }
}

impl<'tcx> TypeFoldable<'tcx> for crate::generic::GenericArg<'tcx> {
    fn fold_with<F: TypeFolder<'tcx>>(self, folder: &mut F) -> Self {
        match self {
            crate::generic::GenericArg::Type(ty) => {
                crate::generic::GenericArg::Type(ty.fold_with(folder))
            }
            crate::generic::GenericArg::Const(ct) => crate::generic::GenericArg::Const(ct),
        }
    }
}

impl<'tcx> TypeFoldable<'tcx> for Const<'tcx> {
    fn fold_with<F: TypeFolder<'tcx>>(self, _folder: &mut F) -> Self {
        self
    }
}
