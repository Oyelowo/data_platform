/*! Predicates — trait bounds, projection equalities, and assumptions. */

use yelang_arena::DefId;

use crate::ty::{ConstId, GenericArgsRef, ImplPolarity, ProjectionTy, TyId};

/// Something that must be proven to hold.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Predicate {
    /// A trait bound: `T: Clone`.
    Trait(TraitPredicate),
    /// An associated type projection equality: `<T as Iterator>::Item == U`.
    Projection(ProjectionPredicate),
    /// A normalization goal: `<T as Iterator>::Item normalizes-to U`.
    NormalizesTo(NormalizesToPredicate),
    /// A well-formedness goal: `T` is well-formed.
    WellFormed(WellFormedPredicate),
    /// A type outlives bound (no-op in Yelang, kept for uniformity).
    TypeOutlives(TypeOutlivesPredicate),
    /// A const expression that must be evaluatable.
    ConstEvaluatable(ConstId),
}

/// A trait bound predicate.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct TraitPredicate {
    pub trait_ref: TraitRef,
    pub polarity: ImplPolarity,
}

impl std::fmt::Debug for TraitPredicate {
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
pub struct TraitRef {
    pub def_id: DefId,
    pub args: GenericArgsRef,
}

/// A projection predicate: `<T as Trait>::Assoc == U`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProjectionPredicate {
    pub projection_ty: ProjectionTy,
    pub term: TyId,
}

impl std::fmt::Debug for ProjectionPredicate {
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
pub struct NormalizesToPredicate {
    pub projection_ty: ProjectionTy,
    pub term: TyId,
}

impl std::fmt::Debug for NormalizesToPredicate {
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
pub struct TypeOutlivesPredicate {
    pub ty: TyId,
}

/// The environment of assumptions available when proving a goal.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ParamEnv {
    pub caller_bounds: ListPredicate,
}

/// An interned list of predicates.
pub type ListPredicate = crate::list::List<Predicate>;

/// A well-formed predicate.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct WellFormedPredicate {
    pub ty: TyId,
}

use std::fmt;

impl fmt::Debug for Predicate {
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

impl fmt::Debug for TraitRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TraitRef({:?})", self.def_id)
    }
}

impl fmt::Debug for ParamEnv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ParamEnv({:?})", self.caller_bounds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interner::Interner;
    use crate::primitive::IntTy;
    use crate::ty::Ty;

    #[test]
    fn trait_predicate_basic() {
        let interner = Interner::new();
        let t_i32 = interner.mk_ty(Ty::Int(IntTy::I32));
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
