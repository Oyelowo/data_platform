/*! Instantiation — turn canonical bound variables back into inference variables.
 *
 * This is the inverse of canonicalization. Given a `Canonical<V>` produced by
 * the canonicalizer, the instantiator creates fresh inference variables (or
 * fresh placeholders) for each canonical variable and shifts the remaining
 * bound variables back in by one binder level.
 */

use yelang_infer::InferCtxt;
use yelang_interner::Symbol;
use yelang_ty::binder::{BoundTy, DebruijnIndex};
use yelang_ty::canonical::{Canonical, CanonicalTyVarKind, CanonicalVarKind, CanonicalVarKinds};
use yelang_ty::fold::{TypeFoldable, TypeFolder, TypeSuperFoldable};
use yelang_ty::interner::Interner;
use yelang_ty::ty::{Const, ConstId, PlaceholderType, Ty, TyId, UniverseIndex};

/// Folds a canonical value, replacing canonical bound variables with fresh
/// inference variables or placeholders.
pub struct InstantiationCtxt<'a> {
    interner: &'a Interner,
    infcx: &'a mut InferCtxt,
    variables: CanonicalVarKinds,
    placeholder_counter: u32,
}

impl<'a> InstantiationCtxt<'a> {
    pub fn new(
        interner: &'a Interner,
        infcx: &'a mut InferCtxt,
        variables: CanonicalVarKinds,
    ) -> Self {
        Self {
            interner,
            infcx,
            variables,
            placeholder_counter: 0,
        }
    }

    fn fresh_placeholder(&mut self, universe: UniverseIndex) -> PlaceholderType {
        let name = Symbol::from(self.placeholder_counter);
        self.placeholder_counter += 1;
        PlaceholderType { universe, name }
    }
}

impl<'a> TypeFolder for InstantiationCtxt<'a> {
    fn interner(&self) -> &Interner {
        self.interner
    }

    fn fold_ty(&mut self, ty: TyId) -> TyId {
        match self.interner.ty(ty) {
            Ty::Bound(debruijn, BoundTy { var, .. })
                if debruijn == DebruijnIndex::INNERMOST =>
            {
                let index = var.0 as usize;
                assert!(
                    index < self.variables.len(),
                    "canonical variable index {} out of range (len {})",
                    index,
                    self.variables.len()
                );
                match self.variables.as_slice()[index] {
                    CanonicalVarKind::Ty(CanonicalTyVarKind::General(_)) => {
                        self.infcx.new_ty_var(self.interner)
                    }
                    CanonicalVarKind::Ty(CanonicalTyVarKind::Int) | CanonicalVarKind::Int => {
                        self.infcx.new_int_var(self.interner)
                    }
                    CanonicalVarKind::Ty(CanonicalTyVarKind::Float) | CanonicalVarKind::Float => {
                        self.infcx.new_float_var(self.interner)
                    }
                    CanonicalVarKind::PlaceholderTy(p) => {
                        let fresh = self.fresh_placeholder(p.universe);
                        self.interner.mk_ty(Ty::Placeholder(fresh))
                    }
                    CanonicalVarKind::Const => {
                        panic!("type variable bound to a const canonical variable")
                    }
                }
            }
            Ty::Bound(debruijn, bound_ty) => self
                .interner
                .mk_ty(Ty::Bound(debruijn.shifted_out(), bound_ty)),
            _ => ty.super_fold_with(self),
        }
    }

    fn fold_const(&mut self, ct: ConstId) -> ConstId {
        let ty = self.interner.const_ty(ct);
        let kind = self.interner.const_kind(ct);
        match kind {
            Const::Bound(debruijn, bound_var) if debruijn == DebruijnIndex::INNERMOST => {
                let index = bound_var.0 as usize;
                assert!(
                    index < self.variables.len(),
                    "canonical const variable index {} out of range (len {})",
                    index,
                    self.variables.len()
                );
                assert!(
                    matches!(self.variables.as_slice()[index], CanonicalVarKind::Const),
                    "expected const canonical variable, got {:?}",
                    self.variables.as_slice()[index]
                );
                let ty = ty.fold_with(self);
                self.infcx.new_const_var(self.interner, ty)
            }
            Const::Bound(debruijn, bound_var) => self.interner.mk_const_from_parts(
                Const::Bound(debruijn.shifted_out(), bound_var),
                ty.fold_with(self),
            ),
            _ => ct.super_fold_with(self),
        }
    }
}

/// Instantiate a canonical value, producing a value with fresh inference
/// variables in place of the canonical bound variables.
pub fn instantiate<V>(
    canonical: Canonical<V>,
    interner: &Interner,
    infcx: &mut InferCtxt,
) -> V
where
    V: TypeFoldable,
{
    let mut instantiator = InstantiationCtxt::new(interner, infcx, canonical.variables);
    canonical.value.fold_with(&mut instantiator)
}
