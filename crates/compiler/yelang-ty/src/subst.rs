/*! Substitution and De Bruijn index shifting.
 *
 * A `Substitution` maps generic parameter indices to concrete generic
 * arguments. `SubstFolder` applies a substitution to types, constants, and
 * predicates.
 *
 * `ShiftBoundVars` shifts De Bruijn indices for higher-ranked types.
 */

use crate::binder::DebruijnIndex;
use crate::fold::{TypeFoldable, TypeFolder, TypeSuperFoldable};
use crate::generic::{GenericArg, Substitution};
use crate::interner::Interner;
use crate::ty::{Binder, Const, ConstId, Ty, TyId};

/// Apply a substitution to a type-like value.
pub fn substitute<T>(interner: &Interner, value: T, subst: &Substitution) -> T
where
    T: TypeFoldable,
{
    value.fold_with(&mut SubstFolder { interner, subst })
}

/// Folder that applies a substitution.
struct SubstFolder<'a> {
    interner: &'a Interner,
    subst: &'a Substitution,
}

impl<'a> TypeFolder for SubstFolder<'a> {
    fn interner(&self) -> &Interner {
        self.interner
    }

    fn fold_ty(&mut self, ty: TyId) -> TyId {
        match self.interner.ty(ty) {
            Ty::Param(param) => match self.subst.get(param.index as usize) {
                Some(GenericArg::Type(replacement)) => replacement.fold_with(self),
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

    fn fold_const(&mut self, ct: ConstId) -> ConstId {
        match self.interner.const_kind(ct) {
            Const::Param(param) => match self.subst.get(param.index as usize) {
                Some(GenericArg::Const(replacement)) => replacement.fold_with(self),
                Some(GenericArg::Type(_)) => {
                    // Const parameter substituted with a type: type-system error.
                    ct
                }
                None => ct,
            },
            _ => ct.super_fold_with(self),
        }
    }
}

// ---------------------------------------------------------------------------
// Bound-variable shifting
// ---------------------------------------------------------------------------

/// Shift all De Bruijn indices in a type-like value out by one binder level.
pub fn shift_out_to_binder<T>(interner: &Interner, value: T) -> T
where
    T: TypeFoldable,
{
    value.fold_with(&mut ShiftBoundVars {
        interner,
        direction: ShiftDirection::Out,
    })
}

/// Shift all De Bruijn indices in a type-like value in by one binder level.
pub fn shift_in<T>(interner: &Interner, value: T) -> T
where
    T: TypeFoldable,
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

struct ShiftBoundVars<'a> {
    interner: &'a Interner,
    direction: ShiftDirection,
}

impl<'a> TypeFolder for ShiftBoundVars<'a> {
    fn interner(&self) -> &Interner {
        self.interner
    }

    fn fold_ty(&mut self, ty: TyId) -> TyId {
        match self.interner.ty(ty) {
            Ty::Bound(debruijn, bound_ty) => {
                let new_index = match self.direction {
                    ShiftDirection::Out => debruijn.shifted_in(),
                    ShiftDirection::In => debruijn.shifted_out(),
                };
                self.interner.mk_ty(Ty::Bound(new_index, bound_ty))
            }
            _ => ty.super_fold_with(self),
        }
    }

    fn fold_const(&mut self, ct: ConstId) -> ConstId {
        match self.interner.const_kind(ct) {
            Const::Bound(debruijn, bound_var) => {
                let new_index = match self.direction {
                    ShiftDirection::Out => debruijn.shifted_in(),
                    ShiftDirection::In => debruijn.shifted_out(),
                };
                let ty = self.interner.const_ty(ct).fold_with(self);
                self.interner
                    .mk_const_from_parts(Const::Bound(new_index, bound_var), ty)
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
pub fn instantiate_binder<T>(interner: &Interner, binder: Binder<T>, args: &[GenericArg]) -> T
where
    T: TypeFoldable + Copy,
{
    let value = binder.value.fold_with(&mut InstantiateBinderFolder {
        interner,
        args,
        binder_index: DebruijnIndex::INNERMOST,
    });
    // After replacing INNERMOST bound vars, shift remaining bound vars in by one.
    shift_in(interner, value)
}

struct InstantiateBinderFolder<'a> {
    interner: &'a Interner,
    args: &'a [GenericArg],
    binder_index: DebruijnIndex,
}

impl<'a> TypeFolder for InstantiateBinderFolder<'a> {
    fn interner(&self) -> &Interner {
        self.interner
    }

    fn fold_ty(&mut self, ty: TyId) -> TyId {
        match self.interner.ty(ty) {
            Ty::Bound(debruijn, bound_ty) if debruijn == self.binder_index => {
                match self.args.get(bound_ty.var.0 as usize) {
                    Some(GenericArg::Type(replacement)) => *replacement,
                    _ => ty,
                }
            }
            _ => ty.super_fold_with(self),
        }
    }

    fn fold_const(&mut self, ct: ConstId) -> ConstId {
        match self.interner.const_kind(ct) {
            Const::Bound(debruijn, bound_var) if debruijn == self.binder_index => {
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
    use crate::binder::{BoundTy, BoundTyKind, BoundVar, BoundVariableKind, DebruijnIndex};
    use crate::generic::GenericArg;
    use crate::interner::Interner;
    use crate::primitive::IntTy;
    use crate::ty::{AdtDef, ConstValue, ParamConst, ParamTy, Ty};
    use yelang_arena::DefId;
    use yelang_interner::Symbol;

    #[test]
    fn substitute_type_param() {
        let interner = Interner::new();
        let t_param = interner.mk_ty(Ty::Param(ParamTy {
            index: 0,
            name: Symbol::from(1),
        }));
        let t_i32 = interner.mk_ty(Ty::Int(IntTy::I32));
        let tuple = interner.mk_ty(Ty::Tuple(
            interner.mk_generic_args(&[GenericArg::Type(t_param)]),
        ));

        let subst = Substitution::from_args(vec![GenericArg::Type(t_i32)]);
        let result = substitute(&interner, tuple, &subst);

        match interner.ty(result) {
            Ty::Tuple(args) => assert_eq!(args[0].expect_type(), t_i32),
            _ => panic!("expected tuple"),
        }
    }

    #[test]
    fn substitute_const_param() {
        let interner = Interner::new();
        let c_param = interner.mk_const_from_parts(
            Const::Param(ParamConst {
                index: 0,
                name: Symbol::from(1),
            }),
            interner.mk_ty(Ty::Int(IntTy::I32)),
        );
        let c_value = interner.mk_const_from_parts(
            Const::Value(ConstValue::Int(42)),
            interner.mk_ty(Ty::Int(IntTy::I32)),
        );
        let array = interner.mk_ty(Ty::Array(interner.mk_ty(Ty::Int(IntTy::I32)), c_param));

        let subst = Substitution::from_args(vec![GenericArg::Const(c_value)]);
        let result = substitute(&interner, array, &subst);

        match interner.ty(result) {
            Ty::Array(_, len) => match interner.const_kind(len) {
                Const::Value(ConstValue::Int(42)) => {}
                _ => panic!("expected const value 42"),
            },
            _ => panic!("expected array"),
        }
    }

    #[test]
    fn substitute_nested_type_param() {
        let interner = Interner::new();
        let t_param_t = interner.mk_ty(Ty::Param(ParamTy {
            index: 0,
            name: Symbol::from(1),
        }));
        let t_param_u = interner.mk_ty(Ty::Param(ParamTy {
            index: 1,
            name: Symbol::from(2),
        }));
        let t_i64 = interner.mk_ty(Ty::Int(IntTy::I64));

        // Vec<T> (represented as Adt with generic arg T)
        let vec_t = interner.mk_ty(Ty::Adt(
            AdtDef {
                def_id: DefId::new(1),
            },
            interner.mk_generic_args(&[GenericArg::Type(t_param_t)]),
        ));

        // Substitute T -> Vec<U>, U -> i64. Expected: Vec<Vec<i64>>.
        let vec_u = interner.mk_ty(Ty::Adt(
            AdtDef {
                def_id: DefId::new(1),
            },
            interner.mk_generic_args(&[GenericArg::Type(t_param_u)]),
        ));
        let subst = Substitution::from_args(vec![GenericArg::Type(vec_u), GenericArg::Type(t_i64)]);
        let result = substitute(&interner, vec_t, &subst);

        match interner.ty(result) {
            Ty::Adt(_, args) => match interner.ty(args[0].expect_type()) {
                Ty::Adt(_, inner_args) => {
                    assert_eq!(inner_args[0].expect_type(), t_i64);
                }
                _ => panic!("expected nested vec"),
            },
            _ => panic!("expected adt"),
        }
    }

    #[test]
    fn shift_bound_vars_out() {
        let interner = Interner::new();
        let bound0 = interner.mk_ty(Ty::Bound(
            DebruijnIndex::INNERMOST,
            BoundTy {
                var: BoundVar(0),
                kind: BoundTyKind::Anon,
            },
        ));
        let shifted = shift_out_to_binder(&interner, bound0);
        match interner.ty(shifted) {
            Ty::Bound(d, _) => assert_eq!(d.0, 1),
            _ => panic!("expected bound var"),
        }
    }

    #[test]
    fn instantiate_binder_replaces_bound_var() {
        let interner = Interner::new();
        let t_i32 = interner.mk_ty(Ty::Int(IntTy::I32));
        let bound0 = interner.mk_ty(Ty::Bound(
            DebruijnIndex::INNERMOST,
            BoundTy {
                var: BoundVar(0),
                kind: BoundTyKind::Anon,
            },
        ));
        let binder = Binder {
            bound_vars: interner.mk_bound_var_list(&[BoundVariableKind::Ty(BoundTy {
                var: BoundVar(0),
                kind: BoundTyKind::Anon,
            })]),
            value: bound0,
        };

        let result = instantiate_binder(&interner, binder, &[GenericArg::Type(t_i32)]);
        assert_eq!(result, t_i32);
    }
}
