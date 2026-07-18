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
use yelang_ty::ty::{Const, ConstId, ConstVid, FloatVid, InferTy, IntVid, PlaceholderType, Ty, TyId, TyVid, UniverseIndex};

/// Mapping from a canonical variable index to the solver variable or
/// placeholder that was created for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanonicalVarMapping {
    Ty(TyVid),
    Int(IntVid),
    Float(FloatVid),
    Const(ConstVid),
    Placeholder(PlaceholderType),
}

/// Folds a canonical value, replacing canonical bound variables with fresh
/// inference variables or placeholders.
pub struct InstantiationCtxt<'a> {
    interner: &'a Interner,
    infcx: &'a mut InferCtxt,
    variables: CanonicalVarKinds,
    placeholder_counter: u32,
    /// Mapping from canonical variable index to the solver variable created for
    /// it, populated in first-occurrence order.
    mapping: Vec<CanonicalVarMapping>,
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
            mapping: Vec::new(),
        }
    }

    fn fresh_placeholder(&mut self, universe: UniverseIndex) -> PlaceholderType {
        let name = Symbol::from(self.placeholder_counter);
        self.placeholder_counter += 1;
        PlaceholderType { universe, name }
    }

    /// Record a mapping for canonical variable `index`.
    fn record_mapping(&mut self, index: usize, mapping: CanonicalVarMapping) {
        if index >= self.mapping.len() {
            self.mapping.resize(index + 1, CanonicalVarMapping::Ty(TyVid(0)));
        }
        self.mapping[index] = mapping;
    }

    /// Returns the mapping from canonical variable index to solver inference
    /// variable, in canonical-variable order.
    pub fn into_mapping(self) -> Vec<CanonicalVarMapping> {
        self.mapping
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
                        let ty = self.infcx.new_ty_var(self.interner);
                        if let Ty::Infer(InferTy::TyVar(vid)) = self.interner.ty(ty) {
                            self.record_mapping(index, CanonicalVarMapping::Ty(vid));
                        }
                        ty
                    }
                    CanonicalVarKind::Ty(CanonicalTyVarKind::Int) | CanonicalVarKind::Int => {
                        let ty = self.infcx.new_int_var(self.interner);
                        if let Ty::Infer(InferTy::IntVar(vid)) = self.interner.ty(ty) {
                            self.record_mapping(index, CanonicalVarMapping::Int(vid));
                        }
                        ty
                    }
                    CanonicalVarKind::Ty(CanonicalTyVarKind::Float) | CanonicalVarKind::Float => {
                        let ty = self.infcx.new_float_var(self.interner);
                        if let Ty::Infer(InferTy::FloatVar(vid)) = self.interner.ty(ty) {
                            self.record_mapping(index, CanonicalVarMapping::Float(vid));
                        }
                        ty
                    }
                    CanonicalVarKind::PlaceholderTy(p) => {
                        let fresh = self.fresh_placeholder(p.universe);
                        self.record_mapping(index, CanonicalVarMapping::Placeholder(fresh));
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
                let ct = self.infcx.new_const_var(self.interner, ty);
                if let Const::Infer(vid) = self.interner.const_kind(ct) {
                    self.record_mapping(index, CanonicalVarMapping::Const(vid));
                }
                ct
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
/// variables in place of the canonical bound variables, and returning the
/// mapping from canonical variable index to solver inference variable.
pub fn instantiate_with_mapping<V>(
    canonical: Canonical<V>,
    interner: &Interner,
    infcx: &mut InferCtxt,
) -> (V, Vec<CanonicalVarMapping>)
where
    V: TypeFoldable,
{
    let mut instantiator = InstantiationCtxt::new(interner, infcx, canonical.variables);
    let value = canonical.value.fold_with(&mut instantiator);
    (value, instantiator.into_mapping())
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
    instantiate_with_mapping(canonical, interner, infcx).0
}
