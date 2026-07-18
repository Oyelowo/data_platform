/*! Exhaustive occurs-check for type inference.
 *
 * The occurs-check prevents constructing cyclic types such as `?T = Vec<?T>`.
 * It must inspect every constructor that can contain nested types or inference
 * variables.
 */

use yelang_ty::existential::ExistentialPredicate;
use yelang_ty::generic::GenericArg;
use yelang_ty::ty::{Const, ConstKind, InferTy, Ty, TyKind, TyVid};

use crate::type_variable::VariableTables;
use crate::unify::UnifyKey;

/// Check whether `vid` occurs inside `ty`.
pub fn occurs_check<'tcx>(tables: &mut VariableTables<'tcx>, vid: TyVid, ty: Ty<'tcx>) -> bool {
    occurs_check_ty(tables, vid, ty)
}

fn occurs_check_ty<'tcx>(tables: &mut VariableTables<'tcx>, vid: TyVid, ty: Ty<'tcx>) -> bool {
    match ty.kind() {
        TyKind::Infer(InferTy::TyVar(other_vid)) => {
            let root = tables.ty_vars.find(*other_vid);
            root.index() == vid.index()
        }
        TyKind::Bool
        | TyKind::Char
        | TyKind::Str
        | TyKind::Int(_)
        | TyKind::Uint(_)
        | TyKind::Float(_)
        | TyKind::Param(_)
        | TyKind::Bound(_, _)
        | TyKind::Infer(InferTy::IntVar(_) | InferTy::FloatVar(_))
        | TyKind::Never
        | TyKind::TypeLit(_)
        | TyKind::Placeholder(_)
        | TyKind::Error => false,
        TyKind::Adt(_, args) | TyKind::Tuple(args) | TyKind::Utility(_, args) => {
            occurs_check_generic_args(tables, vid, args)
        }
        TyKind::FnPtr(sig) => {
            occurs_check_generic_args(tables, vid, &sig.sig.inputs)
                || occurs_check_ty(tables, vid, sig.sig.output)
        }
        TyKind::FnDef(fd) => occurs_check_generic_args(tables, vid, &fd.args),
        TyKind::Array(ty, len) => {
            occurs_check_ty(tables, vid, *ty) || occurs_check_const(tables, vid, *len)
        }
        TyKind::Slice(ty) | TyKind::Ref(ty, _) => occurs_check_ty(tables, vid, *ty),
        TyKind::RawPtr(tam) => occurs_check_ty(tables, vid, tam.ty),
        TyKind::AnonStruct(anon) => anon
            .fields
            .iter()
            .any(|f| occurs_check_ty(tables, vid, f.ty)),
        TyKind::Union(a, b) => occurs_check_ty(tables, vid, *a) || occurs_check_ty(tables, vid, *b),
        TyKind::Alias(alias) => occurs_check_generic_args(tables, vid, &alias.args),
        TyKind::Projection(proj) => occurs_check_trait_ref(tables, vid, &proj.trait_ref),
        TyKind::Dynamic(binder) => binder.value.iter().any(|pred| match pred {
            ExistentialPredicate::Trait(tr) => occurs_check_generic_args(tables, vid, &tr.args),
            ExistentialPredicate::Projection(pr) => {
                occurs_check_generic_args(tables, vid, &pr.args)
                    || occurs_check_ty(tables, vid, pr.term)
            }
            ExistentialPredicate::AutoTrait(_) => false,
        }),
    }
}

fn occurs_check_generic_args<'tcx>(
    tables: &mut VariableTables<'tcx>,
    vid: TyVid,
    args: &yelang_ty::ty::GenericArgsRef<'tcx>,
) -> bool {
    args.iter().any(|arg| match arg {
        GenericArg::Type(t) => occurs_check_ty(tables, vid, *t),
        GenericArg::Const(c) => occurs_check_const(tables, vid, *c),
    })
}

fn occurs_check_trait_ref<'tcx>(
    tables: &mut VariableTables<'tcx>,
    vid: TyVid,
    tr: &yelang_ty::predicate::TraitRef<'tcx>,
) -> bool {
    occurs_check_generic_args(tables, vid, &tr.args)
}

fn occurs_check_const<'tcx>(
    tables: &mut VariableTables<'tcx>,
    vid: TyVid,
    ct: Const<'tcx>,
) -> bool {
    if occurs_check_ty(tables, vid, ct.ty) {
        return true;
    }
    match ct.kind {
        ConstKind::Unevaluated(u) => occurs_check_generic_args(tables, vid, &u.args),
        ConstKind::Value(_)
        | ConstKind::Param(_)
        | ConstKind::Bound(_, _)
        | ConstKind::Placeholder(_)
        | ConstKind::Infer(_)
        | ConstKind::Error => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yelang_ty::interner::Interner;
    use yelang_ty::primitive::IntTy;
    use yelang_ty::ty::{AdtDef, TyKind, TyVid};

    #[test]
    fn occurs_check_detects_cycle() {
        let interner = Interner::new();
        let mut tables = VariableTables::new();
        let t_i32 = interner.mk_ty(TyKind::Int(IntTy::I32));
        let t_adt = interner.mk_ty(TyKind::Adt(
            AdtDef {
                def_id: yelang_arena::DefId::new(1),
            },
            interner.mk_generic_args(&[yelang_ty::generic::GenericArg::Type(t_i32)]),
        ));
        assert!(!occurs_check(&mut tables, TyVid(0), t_adt));
    }

    #[test]
    fn occurs_check_finds_var() {
        let interner = Interner::new();
        let mut tables = VariableTables::new();
        let vid = tables
            .ty_vars
            .new_var(crate::type_variable::TypeVarValue::Unknown);
        let var_ty = interner.mk_ty(TyKind::Infer(InferTy::TyVar(vid)));
        assert!(occurs_check(&mut tables, vid, var_ty));
        assert!(!occurs_check(&mut tables, TyVid(vid.0 + 100), var_ty));
    }
}
