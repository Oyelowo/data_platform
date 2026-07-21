/*! Canonicalization — turn inference variables into bound variables.
 *
 * A canonical value has all free inference variables and placeholders replaced
 * by bound variables at the outermost binder. This makes goals cacheable:
 * `Vec<?T>: Clone` and `Vec<?U>: Clone` both canonicalize to
 * `exists<T> Vec<T>: Clone`.
 */

use yelang_arena::FxHashMap;
use yelang_infer::{ConstVarValue, FloatVarValue, InferCtxt, IntVarValue, TypeVarValue};
use yelang_ty::primitive::IntegerTy;
use yelang_ty::binder::{BoundTy, BoundTyKind, BoundVar, DebruijnIndex};
use yelang_ty::canonical::{Canonical, CanonicalTyVarKind, CanonicalVarKind};
use yelang_ty::fold::{TypeFoldable, TypeFolder, TypeSuperFoldable};
use yelang_ty::interner::Interner;

use crate::goal::Goal;
use yelang_ty::ty::{
    Const, ConstId, ConstVid, FloatVid, InferTy, IntVid, PlaceholderType, Ty, TyId, TyVid,
    UniverseIndex,
};

use crate::response::CanonicalGoal;

/// A key identifying a variable that should be canonicalized.
///
/// Two occurrences of the same inference variable must map to the same
/// canonical bound variable.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum CanonicalVarKey {
    TyVar(TyVid),
    IntVar(IntVid),
    FloatVar(FloatVid),
    ConstVar(ConstVid),
    PlaceholderTy(PlaceholderType),
}

/// Folds a value, replacing free variables with canonical bound variables.
pub struct Canonicalizer<'a> {
    interner: &'a Interner,
    infcx: &'a mut InferCtxt,
    max_universe: UniverseIndex,
    variables: Vec<CanonicalVarKind>,
    var_map: FxHashMap<CanonicalVarKey, BoundVar>,
}

impl<'a> Canonicalizer<'a> {
    pub fn new(
        interner: &'a Interner,
        infcx: &'a mut InferCtxt,
        max_universe: UniverseIndex,
    ) -> Self {
        Self {
            interner,
            infcx,
            max_universe,
            variables: Vec::new(),
            var_map: FxHashMap::new(),
        }
    }

    /// Return the canonical bound variable for `key`, creating a new one if
    /// necessary. The first occurrence receives index 0, the next index 1, etc.
    fn canonical_var(&mut self, key: CanonicalVarKey) -> BoundVar {
        if let Some(&index) = self.var_map.get(&key) {
            return index;
        }
        let index = BoundVar(self.variables.len() as u32);
        self.variables.push(self.kind_for_key(&key));
        self.var_map.insert(key, index);
        index
    }

    fn kind_for_key(&self, key: &CanonicalVarKey) -> CanonicalVarKind {
        match key {
            CanonicalVarKey::TyVar(_) => {
                CanonicalVarKind::Ty(CanonicalTyVarKind::General(self.max_universe))
            }
            CanonicalVarKey::IntVar(_) => CanonicalVarKind::Int,
            CanonicalVarKey::FloatVar(_) => CanonicalVarKind::Float,
            CanonicalVarKey::ConstVar(_) => CanonicalVarKind::Const,
            CanonicalVarKey::PlaceholderTy(p) => CanonicalVarKind::PlaceholderTy(*p),
        }
    }
}

impl<'a> TypeFolder for Canonicalizer<'a> {
    fn interner(&self) -> &Interner {
        self.interner
    }

    fn fold_ty(&mut self, ty: TyId) -> TyId {
        match self.interner.ty(ty) {
            Ty::Infer(InferTy::TyVar(vid)) => {
                let root = self.infcx.find_ty_var(vid);
                match self.infcx.probe_ty_var(root) {
                    TypeVarValue::Known(known) => known.fold_with(self),
                    TypeVarValue::Unknown => {
                        let index = self.canonical_var(CanonicalVarKey::TyVar(root));
                        self.interner.mk_ty(Ty::Bound(
                            DebruijnIndex::INNERMOST,
                            BoundTy {
                                var: index,
                                kind: BoundTyKind::Anon,
                            },
                        ))
                    }
                }
            }
            Ty::Infer(InferTy::IntVar(vid)) => {
                let root = self.infcx.find_int_var(vid);
                match self.infcx.probe_int_var(root) {
                    IntVarValue::Known(IntegerTy::Signed(it)) => self.interner.mk_ty(Ty::Int(*it)),
                    IntVarValue::Known(IntegerTy::Unsigned(ut)) => {
                        self.interner.mk_ty(Ty::Uint(*ut))
                    }
                    IntVarValue::Unknown => {
                        let index = self.canonical_var(CanonicalVarKey::IntVar(root));
                        self.interner.mk_ty(Ty::Bound(
                            DebruijnIndex::INNERMOST,
                            BoundTy {
                                var: index,
                                kind: BoundTyKind::Anon,
                            },
                        ))
                    }
                }
            }
            Ty::Infer(InferTy::FloatVar(vid)) => {
                let root = self.infcx.find_float_var(vid);
                match self.infcx.probe_float_var(root) {
                    FloatVarValue::Known(ft) => self.interner.mk_ty(Ty::Float(*ft)),
                    FloatVarValue::Unknown => {
                        let index = self.canonical_var(CanonicalVarKey::FloatVar(root));
                        self.interner.mk_ty(Ty::Bound(
                            DebruijnIndex::INNERMOST,
                            BoundTy {
                                var: index,
                                kind: BoundTyKind::Anon,
                            },
                        ))
                    }
                }
            }
            Ty::Placeholder(placeholder) => {
                let index = self.canonical_var(CanonicalVarKey::PlaceholderTy(placeholder));
                self.interner.mk_ty(Ty::Bound(
                    DebruijnIndex::INNERMOST,
                    BoundTy {
                        var: index,
                        kind: BoundTyKind::Anon,
                    },
                ))
            }
            Ty::Bound(debruijn, bound_ty) => {
                // The canonical binder becomes the new outermost binder, so
                // existing bound variables shift out by one level.
                self.interner
                    .mk_ty(Ty::Bound(debruijn.shifted_in(), bound_ty))
            }
            _ => ty.super_fold_with(self),
        }
    }

    fn fold_const(&mut self, ct: ConstId) -> ConstId {
        let ty = self.interner.const_ty(ct);
        let kind = self.interner.const_kind(ct);
        match kind {
            Const::Infer(vid) => {
                let root = self.infcx.find_const_var(vid);
                match self.infcx.probe_const_var(root) {
                    ConstVarValue::Known(known) => known.fold_with(self),
                    ConstVarValue::Unknown => {
                        let index = self.canonical_var(CanonicalVarKey::ConstVar(root));
                        self.interner.mk_const_from_parts(
                            Const::Bound(DebruijnIndex::INNERMOST, BoundVar(index.0)),
                            ty.fold_with(self),
                        )
                    }
                }
            }
            Const::Bound(debruijn, bound_var) => self.interner.mk_const_from_parts(
                Const::Bound(debruijn.shifted_in(), bound_var),
                ty.fold_with(self),
            ),
            _ => ct.super_fold_with(self),
        }
    }
}

/// Canonicalize any `TypeFoldable` value.
///
/// Inference variables become bound variables; placeholders become placeholder
/// canonical variables. Existing bound variables are shifted out by one binder
/// level to make room for the new outer binder.
pub fn canonicalize<V>(
    value: V,
    interner: &Interner,
    infcx: &mut InferCtxt,
    max_universe: UniverseIndex,
) -> Canonical<V>
where
    V: TypeFoldable,
{
    let mut canonicalizer = Canonicalizer::new(interner, infcx, max_universe);
    let value = value.fold_with(&mut canonicalizer);
    let variables = interner.mk_canonical_var_kinds(&canonicalizer.variables);
    Canonical::new(value, max_universe, variables)
}

/// Canonicalize a solver goal.
pub fn canonicalize_goal(
    goal: Goal,
    interner: &Interner,
    infcx: &mut InferCtxt,
    max_universe: UniverseIndex,
) -> CanonicalGoal {
    canonicalize(goal, interner, infcx, max_universe)
}
