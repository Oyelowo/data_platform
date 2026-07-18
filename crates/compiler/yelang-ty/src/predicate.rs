/*! Predicates — trait bounds, projection equalities, and assumptions. */

use yelang_arena::DefId;

use crate::ty::{Const, GenericArgsRef, ImplPolarity, ProjectionTy, Ty};

/// Something that must be proven to hold.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Predicate<'tcx> {
    /// A trait bound: `T: Clone`.
    Trait(TraitPredicate<'tcx>),
    /// An associated type projection equality: `<T as Iterator>::Item == U`.
    Projection(ProjectionPredicate<'tcx>),
    /// A normalization goal: `<T as Iterator>::Item normalizes-to U`.
    NormalizesTo(NormalizesToPredicate<'tcx>),
    /// A well-formedness goal: `T` is well-formed.
    WellFormed(WellFormedPredicate<'tcx>),
    /// A type outlives bound (no-op in Yelang, kept for uniformity).
    TypeOutlives(TypeOutlivesPredicate<'tcx>),
    /// A const expression that must be evaluatable.
    ConstEvaluatable(Const<'tcx>),
}

/// A trait bound predicate.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct TraitPredicate<'tcx> {
    pub trait_ref: TraitRef<'tcx>,
    pub polarity: ImplPolarity,
}

impl<'tcx> std::fmt::Debug for TraitPredicate<'tcx> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "TraitPredicate({:?}, {:?})",
            self.trait_ref.def_id, self.polarity
        )
    }
}

/// A trait reference: `Clone` in `T: Clone`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct TraitRef<'tcx> {
    pub def_id: DefId,
    pub args: GenericArgsRef<'tcx>,
}

/// A projection predicate: `<T as Trait>::Assoc == U`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProjectionPredicate<'tcx> {
    pub projection_ty: ProjectionTy<'tcx>,
    pub term: Ty<'tcx>,
}

impl<'tcx> std::fmt::Debug for ProjectionPredicate<'tcx> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ProjectionPredicate({:?} == {:?})",
            self.projection_ty.item_def_id, self.term
        )
    }
}

/// A normalization predicate: `<T as Trait>::Assoc normalizes-to U`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct NormalizesToPredicate<'tcx> {
    pub projection_ty: ProjectionTy<'tcx>,
    pub term: Ty<'tcx>,
}

impl<'tcx> std::fmt::Debug for NormalizesToPredicate<'tcx> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "NormalizesTo({:?}, {:?})",
            self.projection_ty.item_def_id, self.term
        )
    }
}

/// A type outlives predicate (no-op in Yelang).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeOutlivesPredicate<'tcx> {
    pub ty: Ty<'tcx>,
}

/// The environment of assumptions available when proving a goal.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ParamEnv<'tcx> {
    pub caller_bounds: ListPredicate<'tcx>,
}

/// An interned list of predicates.
pub type ListPredicate<'tcx> = crate::list::List<Predicate<'tcx>>;

/// A well-formed predicate.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct WellFormedPredicate<'tcx> {
    pub ty: Ty<'tcx>,
}

use std::fmt;

impl<'tcx> fmt::Debug for Predicate<'tcx> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Predicate::Trait(tp) => {
                write!(f, "Trait({:?}, {:?})", tp.trait_ref.def_id, tp.polarity)
            }
            Predicate::Projection(pp) => {
                write!(
                    f,
                    "Projection({:?} == {:?})",
                    pp.projection_ty.item_def_id, pp.term
                )
            }
            Predicate::NormalizesTo(np) => {
                write!(
                    f,
                    "NormalizesTo({:?}, {:?})",
                    np.projection_ty.item_def_id, np.term
                )
            }
            Predicate::WellFormed(wf) => write!(f, "WellFormed({:?})", wf.ty),
            Predicate::TypeOutlives(_) => write!(f, "TypeOutlives"),
            Predicate::ConstEvaluatable(ct) => write!(f, "ConstEvaluatable({:?})", ct),
        }
    }
}

impl<'tcx> fmt::Debug for TraitRef<'tcx> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TraitRef({:?})", self.def_id)
    }
}

impl<'tcx> fmt::Debug for ParamEnv<'tcx> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ParamEnv({:?})", self.caller_bounds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interner::Interner;
    use crate::primitive::IntTy;
    use crate::ty::TyKind;

    #[test]
    fn trait_predicate_basic() {
        let interner = Interner::new();
        let t_i32 = interner.mk_ty(TyKind::Int(IntTy::I32));
        let args = interner.mk_generic_args(&[crate::generic::GenericArg::Type(t_i32)]);
        let trait_ref = TraitRef {
            def_id: DefId::new(1),
            args,
        };
        let pred = Predicate::Trait(TraitPredicate {
            trait_ref,
            polarity: ImplPolarity::Positive,
        });
        match pred {
            Predicate::Trait(tp) => {
                assert_eq!(tp.trait_ref.def_id.raw(), 1);
                assert_eq!(tp.polarity, ImplPolarity::Positive);
            }
            _ => panic!("expected Trait predicate"),
        }
    }
}
