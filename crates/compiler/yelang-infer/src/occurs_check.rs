/*! Exhaustive occurs-check for type inference.
 *
 * The occurs-check prevents constructing cyclic types such as `?T = Vec<?T>`.
 * It must inspect every constructor that can contain nested types or inference
 * variables.
 */

use yelang_ty::existential::ExistentialPredicate;
use yelang_ty::generic::GenericArg;
use yelang_ty::interner::Interner;
use yelang_ty::ty::{Const, InferTy, Ty, TyId, TyVid};

use crate::type_variable::VariableTables;
use crate::unify::UnifyKey;

/// Check whether `vid` occurs inside `ty`.
pub fn occurs_check(
    interner: &Interner,
    tables: &mut VariableTables,
    vid: TyVid,
    ty: TyId,
) -> bool {
    occurs_check_ty(interner, tables, vid, ty)
}

fn occurs_check_ty(
    interner: &Interner,
    tables: &mut VariableTables,
    vid: TyVid,
    ty: TyId,
) -> bool {
    match interner.ty(ty) {
        Ty::Infer(InferTy::TyVar(other_vid)) => {
            let root = tables.ty_vars.find(other_vid);
            root.index() == vid.index()
        }
        Ty::Bool
        | Ty::Char
        | Ty::Str
        | Ty::Int(_)
        | Ty::Uint(_)
        | Ty::Float(_)
        | Ty::Param(_)
        | Ty::Bound(_, _)
        | Ty::Infer(InferTy::IntVar(_) | InferTy::FloatVar(_))
        | Ty::Never
        | Ty::TypeLit(_)
        | Ty::Placeholder(_)
        | Ty::Error => false,
        Ty::Adt(_, args) | Ty::Tuple(args) | Ty::Utility(_, args) => {
            occurs_check_generic_args(interner, tables, vid, &args)
        }
        Ty::FnPtr(sig) => {
            occurs_check_generic_args(interner, tables, vid, &sig.sig.inputs)
                || occurs_check_ty(interner, tables, vid, sig.sig.output)
        }
        Ty::FnDef(fd) => occurs_check_generic_args(interner, tables, vid, &fd.args),
        Ty::Array(ty, len) => {
            occurs_check_ty(interner, tables, vid, ty)
                || occurs_check_const(interner, tables, vid, len)
        }
        Ty::Slice(ty) | Ty::Ref(ty, _) => {
            occurs_check_ty(interner, tables, vid, ty)
        }
        Ty::RawPtr(tam) => occurs_check_ty(interner, tables, vid, tam.ty),
        Ty::AnonStruct(anon) => anon
            .fields
            .iter()
            .any(|f| occurs_check_ty(interner, tables, vid, f.ty)),
        Ty::Union(a, b) => {
            occurs_check_ty(interner, tables, vid, a)
                || occurs_check_ty(interner, tables, vid, b)
        }
        Ty::Alias(alias) => occurs_check_generic_args(interner, tables, vid, &alias.args),
        Ty::Projection(proj) => {
            occurs_check_trait_ref(interner, tables, vid, &proj.trait_ref)
        }
        Ty::Dynamic(binder) => binder.value.iter().any(|pred| match pred {
            ExistentialPredicate::Trait(tr) => {
                occurs_check_generic_args(interner, tables, vid, &tr.args)
            }
            ExistentialPredicate::Projection(pr) => {
                occurs_check_generic_args(interner, tables, vid, &pr.args)
                    || occurs_check_ty(interner, tables, vid, pr.term)
            }
            ExistentialPredicate::AutoTrait(_) => false,
        }),
    }
}

fn occurs_check_generic_args(
    interner: &Interner,
    tables: &mut VariableTables,
    vid: TyVid,
    args: &yelang_ty::ty::GenericArgsRef,
) -> bool {
    args.iter().any(|arg| match arg {
        GenericArg::Type(t) => occurs_check_ty(interner, tables, vid, *t),
        GenericArg::Const(c) => occurs_check_const(interner, tables, vid, *c),
    })
}

fn occurs_check_trait_ref(
    interner: &Interner,
    tables: &mut VariableTables,
    vid: TyVid,
    tr: &yelang_ty::predicate::TraitRef,
) -> bool {
    occurs_check_generic_args(interner, tables, vid, &tr.args)
}

fn occurs_check_const(
    interner: &Interner,
    tables: &mut VariableTables,
    vid: TyVid,
    ct: yelang_ty::ty::ConstId,
) -> bool {
    if occurs_check_ty(interner, tables, vid, interner.const_ty(ct)) {
        return true;
    }
    match interner.const_kind(ct) {
        Const::Unevaluated(u) => occurs_check_generic_args(interner, tables, vid, &u.args),
        Const::Value(_)
        | Const::Param(_)
        | Const::Bound(_, _)
        | Const::Placeholder(_)
        | Const::Infer(_)
        | Const::Error => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yelang_ty::primitive::IntTy;
    use yelang_ty::ty::{AdtDef, Ty, TyVid};

    #[test]
    fn occurs_check_detects_cycle() {
        let interner = Interner::new();
        let mut tables = VariableTables::new();
        let t_i32 = interner.mk_ty(Ty::Int(IntTy::I32));
        let t_adt = interner.mk_ty(Ty::Adt(
            AdtDef {
                def_id: yelang_arena::DefId::new(1),
            },
            interner.mk_generic_args(&[yelang_ty::generic::GenericArg::Type(t_i32)]),
        ));
        assert!(!occurs_check(&interner, &mut tables, TyVid(0), t_adt));
    }

    #[test]
    fn occurs_check_finds_var() {
        let interner = Interner::new();
        let mut tables = VariableTables::new();
        let vid = tables
            .ty_vars
            .new_var(crate::type_variable::TypeVarValue::Unknown);
        let var_ty = interner.mk_ty(Ty::Infer(InferTy::TyVar(vid)));
        assert!(occurs_check(&interner, &mut tables, vid, var_ty));
        assert!(!occurs_check(
            &interner,
            &mut tables,
            TyVid(vid.0 + 100),
            var_ty
        ));
    }
}
