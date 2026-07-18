/*! Substitution and De Bruijn index shifting.
 *
 * A `Substitution` maps generic parameter indices to concrete generic
 * arguments. `SubstFolder` applies a substitution to types, constants, and
 * predicates.
 *
 * `ShiftBoundVars` shifts De Bruijn indices for higher-ranked types.
 */

use std::marker::PhantomData;

use crate::binder::{BoundTy, BoundVar, BoundVariableKind, DebruijnIndex};
use crate::fold::{TypeFoldable, TypeFolder, TypeSuperFoldable};
use crate::generic::{GenericArg, Substitution};
use crate::interner::Interner;
use crate::ty::{Binder, Const, ConstKind, ParamConst, ParamTy, Ty, TyKind};

/// Apply a substitution to a type-like value.
pub fn substitute<'tcx, T>(
    interner: &'tcx Interner<'tcx>,
    value: T,
    subst: &Substitution<'tcx>,
) -> T
where
    T: TypeFoldable<'tcx>,
{
    value.fold_with(&mut SubstFolder { interner, subst })
}

/// Folder that applies a substitution.
struct SubstFolder<'a, 'tcx> {
    interner: &'tcx Interner<'tcx>,
    subst: &'a Substitution<'tcx>,
}

impl<'a, 'tcx> TypeFolder<'tcx> for SubstFolder<'a, 'tcx> {
    fn interner(&self) -> &'tcx Interner<'tcx> {
        self.interner
    }

    fn fold_ty(&mut self, ty: Ty<'tcx>) -> Ty<'tcx> {
        match ty.kind() {
            TyKind::Param(param) => match self.subst.get(param.index as usize) {
                Some(GenericArg::Type(replacement)) => replacement,
                Some(GenericArg::Const(_)) => {
                    // Type parameter substituted with a constant: this is a
                    // type-system error, but we return the original type to
                    // allow error reporting elsewhere.
                    ty
                }
                None => ty,
            },
            _ => ty.super_fold_with(self),
        }
    }

    fn fold_const(&mut self, ct: Const<'tcx>) -> Const<'tcx> {
        match ct.kind {
            ConstKind::Param(param) => match self.subst.get(param.index as usize) {
                Some(GenericArg::Const(replacement)) => replacement,
                Some(GenericArg::Type(_)) => {
                    // Const parameter substituted with a type: type-system error.
                    ct
                }
                None => ct,
            },
            _ => ct,
        }
    }
}

// ---------------------------------------------------------------------------
// Bound-variable shifting
// ---------------------------------------------------------------------------

/// Shift all De Bruijn indices in a type-like value out by one binder level.
pub fn shift_out_to_binder<'tcx, T>(interner: &'tcx Interner<'tcx>, value: T) -> T
where
    T: TypeFoldable<'tcx>,
{
    value.fold_with(&mut ShiftBoundVars {
        interner,
        direction: ShiftDirection::Out,
    })
}

/// Shift all De Bruijn indices in a type-like value in by one binder level.
pub fn shift_in<'tcx, T>(interner: &'tcx Interner<'tcx>, value: T) -> T
where
    T: TypeFoldable<'tcx>,
{
    value.fold_with(&mut ShiftBoundVars {
        interner,
        direction: ShiftDirection::In,
    })
}

enum ShiftDirection {
    Out,
    In,
}

struct ShiftBoundVars<'tcx> {
    interner: &'tcx Interner<'tcx>,
    direction: ShiftDirection,
}

impl<'tcx> TypeFolder<'tcx> for ShiftBoundVars<'tcx> {
    fn interner(&self) -> &'tcx Interner<'tcx> {
        self.interner
    }

    fn fold_ty(&mut self, ty: Ty<'tcx>) -> Ty<'tcx> {
        match ty.kind() {
            TyKind::Bound(debruijn, bound_ty) => {
                let new_index = match self.direction {
                    ShiftDirection::Out => debruijn.shifted_in(),
                    ShiftDirection::In => debruijn.shifted_out(),
                };
                self.interner.mk_ty(TyKind::Bound(new_index, *bound_ty))
            }
            _ => ty.super_fold_with(self),
        }
    }

    fn fold_const(&mut self, ct: Const<'tcx>) -> Const<'tcx> {
        match ct.kind {
            ConstKind::Bound(debruijn, bound_var) => {
                let new_index = match self.direction {
                    ShiftDirection::Out => debruijn.shifted_in(),
                    ShiftDirection::In => debruijn.shifted_out(),
                };
                Const {
                    kind: ConstKind::Bound(new_index, bound_var),
                    ty: ct.ty.fold_with(self),
                }
            }
            _ => ct,
        }
    }
}

// ---------------------------------------------------------------------------
// Binder stripping / instantiation
// ---------------------------------------------------------------------------

/// Instantiate a binder by replacing its bound variables with the given
/// arguments. This removes the outermost binder.
///
/// The caller must ensure that `args` provides one argument for each bound
/// variable in the binder's `bound_vars`.
pub fn instantiate_binder<'tcx, T>(
    interner: &'tcx Interner<'tcx>,
    binder: Binder<'tcx, T>,
    args: &[GenericArg<'tcx>],
) -> T
where
    T: TypeFoldable<'tcx> + Copy + 'tcx,
{
    let value = binder.value.fold_with(&mut InstantiateBinderFolder {
        interner,
        args,
        binder_index: DebruijnIndex::INNERMOST,
    });
    // After replacing INNERMOST bound vars, shift remaining bound vars in by one.
    shift_in(interner, value)
}

struct InstantiateBinderFolder<'a, 'tcx> {
    interner: &'tcx Interner<'tcx>,
    args: &'a [GenericArg<'tcx>],
    binder_index: DebruijnIndex,
}

impl<'a, 'tcx> TypeFolder<'tcx> for InstantiateBinderFolder<'a, 'tcx> {
    fn interner(&self) -> &'tcx Interner<'tcx> {
        self.interner
    }

    fn fold_ty(&mut self, ty: Ty<'tcx>) -> Ty<'tcx> {
        match ty.kind() {
            TyKind::Bound(debruijn, bound_ty) if *debruijn == self.binder_index => {
                match self.args.get(bound_ty.var.0 as usize) {
                    Some(GenericArg::Type(replacement)) => *replacement,
                    _ => ty,
                }
            }
            _ => ty.super_fold_with(self),
        }
    }

    fn fold_const(&mut self, ct: Const<'tcx>) -> Const<'tcx> {
        match ct.kind {
            ConstKind::Bound(debruijn, bound_var)
                if debruijn == self.binder_index =>
            {
                match self.args.get(bound_var.0 as usize) {
                    Some(GenericArg::Const(replacement)) => *replacement,
                    _ => ct,
                }
            }
            _ => ct,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generic::GenericArg;
    use crate::interner::Interner;
    use crate::binder::{BoundTy, BoundTyKind, BoundVar, BoundVariableKind, DebruijnIndex};
    use crate::primitive::IntTy;
    use crate::ty::{ConstValue, ParamConst, ParamTy, TyKind};
    use yelang_interner::Symbol;

    #[test]
    fn substitute_type_param() {
        let interner = Interner::new();
        let t_param = interner.mk_ty(TyKind::Param(ParamTy {
            index: 0,
            name: Symbol::from(1),
        }));
        let t_i32 = interner.mk_ty(TyKind::Int(IntTy::I32));
        let tuple = interner.mk_ty(TyKind::Tuple(
            interner.mk_generic_args(&[GenericArg::Type(t_param)]),
        ));

        let subst = Substitution::from_args(vec![GenericArg::Type(t_i32)]);
        let result = substitute(&interner, tuple, &subst);

        match result.kind() {
            TyKind::Tuple(args) => assert_eq!(args[0].expect_type(), t_i32),
            _ => panic!("expected tuple"),
        }
    }

    #[test]
    fn substitute_const_param() {
        let interner = Interner::new();
        let c_param = Const {
            kind: ConstKind::Param(ParamConst {
                index: 0,
                name: Symbol::from(1),
            }),
            ty: interner.mk_ty(TyKind::Int(IntTy::I32)),
        };
        let c_value = Const {
            kind: ConstKind::Value(crate::ty::ConstValue::Int(42)),
            ty: interner.mk_ty(TyKind::Int(IntTy::I32)),
        };
        let array = interner.mk_ty(TyKind::Array(
            interner.mk_ty(TyKind::Int(IntTy::I32)),
            c_param,
        ));

        let subst = Substitution::from_args(vec![GenericArg::Const(c_value)]);
        let result = substitute(&interner, array, &subst);

        match result.kind() {
            TyKind::Array(_, len) => match len.kind {
                ConstKind::Value(crate::ty::ConstValue::Int(42)) => {}
                _ => panic!("expected const value 42"),
            },
            _ => panic!("expected array"),
        }
    }

    #[test]
    fn shift_bound_vars_out() {
        let interner = Interner::new();
        let bound0 = interner.mk_ty(TyKind::Bound(
            DebruijnIndex::INNERMOST,
            BoundTy {
                var: BoundVar(0),
                kind: BoundTyKind::Anon,
            },
        ));
        let shifted = shift_out_to_binder(&interner, bound0);
        match shifted.kind() {
            TyKind::Bound(d, _) => assert_eq!(d.0, 1),
            _ => panic!("expected bound var"),
        }
    }

    #[test]
    fn instantiate_binder_replaces_bound_var() {
        let interner = Interner::new();
        let t_i32 = interner.mk_ty(TyKind::Int(IntTy::I32));
        let bound0 = interner.mk_ty(TyKind::Bound(
            DebruijnIndex::INNERMOST,
            BoundTy {
                var: BoundVar(0),
                kind: BoundTyKind::Anon,
            },
        ));
        let binder = Binder {
            bound_vars: interner.mk_bound_var_list(&[crate::binder::BoundVariableKind::Ty(
                BoundTy {
                    var: BoundVar(0),
                    kind: BoundTyKind::Anon,
                },
            )]),
            value: bound0,
            _marker: PhantomData,
        };

        let result = instantiate_binder(&interner, binder, &[GenericArg::Type(t_i32)]);
        assert_eq!(result, t_i32);
    }
}
