/*! Lower HIR types to canonical `yelang_ty::Ty`.
 *
 * HIR types (`hir::Ty`) are syntactic and already have resolved paths.
 * This module converts them to the interned type representation.
 */

use yelang_arena::DefId;
use yelang_hir::hir_ty::{
    AnonField as HirAnonField, Ty as HirTy, TyKind as HirTyKind, UtilityKind as HirUtilityKind,
};
use yelang_hir::res::{FloatTy as HirFloatTy, IntTy as HirIntTy, PrimTy, Res};
use yelang_ty::generic::GenericArg;
use yelang_ty::interner::Interner;
use yelang_ty::primitive::{FloatTy, IntTy, UintTy};
use yelang_ty::ty::{
    AdtDef, AliasTy, AnonField, AnonStructDef, ConstKind, Mutability, Ty, TyKind, TypeAndMut,
};

use crate::fn_ctxt::FnCtxt;

/// Lower a HIR type to a canonical type.
pub fn lower_hir_ty<'tcx>(hir_ty: &HirTy, fcx: &mut FnCtxt<'tcx>) -> Ty<'tcx> {
    lower_hir_ty_kind(&hir_ty.kind, fcx)
}

fn lower_hir_ty_kind<'tcx>(kind: &HirTyKind, fcx: &mut FnCtxt<'tcx>) -> Ty<'tcx> {
    let interner = fcx.interner;

    match kind {
        HirTyKind::Path { res } => lower_res(res, fcx),
        HirTyKind::Tuple { tys } => {
            let lowered: Vec<_> = tys.iter().map(|t| lower_hir_ty(t, fcx)).collect();
            let args = interner.mk_generic_args(
                &lowered
                    .iter()
                    .map(|&t| GenericArg::Type(t))
                    .collect::<Vec<_>>(),
            );
            interner.mk_ty(TyKind::Tuple(args))
        }
        HirTyKind::Array { ty, len } => {
            let elem_ty = lower_hir_ty(ty, fcx);
            // TODO: lower array length const properly
            let len_const = yelang_ty::ty::Const {
                kind: ConstKind::Error,
                ty: elem_ty,
            };
            interner.mk_ty(TyKind::Array(elem_ty, len_const))
        }
        HirTyKind::Slice { ty } => {
            let elem_ty = lower_hir_ty(ty, fcx);
            interner.mk_ty(TyKind::Slice(elem_ty))
        }
        HirTyKind::FnPtr { sig } => {
            let inputs = interner.mk_generic_args(
                &sig.inputs
                    .iter()
                    .map(|t| GenericArg::Type(lower_hir_ty(t, fcx)))
                    .collect::<Vec<_>>(),
            );
            let output = lower_hir_ty(&sig.output, fcx);
            interner.mk_ty(TyKind::FnPtr(yelang_ty::ty::PolyFnSig {
                sig: yelang_ty::ty::FnSig { inputs, output },
            }))
        }
        HirTyKind::AnonStruct { fields } => {
            let lowered_fields: Vec<_> = fields
                .iter()
                .map(|f| AnonField {
                    name: f.name,
                    ty: lower_hir_ty(&f.ty, fcx),
                })
                .collect();
            let field_list = interner.mk_bound_var_list(&[]); // placeholder
            // Actually AnonStructDef uses List<AnonField> not bound vars
            // We need to use mk_generic_args or a similar mechanism
            // For now, use from_slice (not interned)
            let fields_list = yelang_ty::list::List::from_slice(&lowered_fields);
            interner.mk_ty(TyKind::AnonStruct(AnonStructDef {
                fields: fields_list,
            }))
        }
        HirTyKind::TypeLit { .. } => {
            // Type literals are union-like; for now return a fresh variable
            fcx.new_ty_var()
        }
        HirTyKind::Utility { kind, args } => {
            let kind = match kind {
                HirUtilityKind::Omit => yelang_ty::ty::UtilityKind::Omit,
                HirUtilityKind::Pick => yelang_ty::ty::UtilityKind::Pick,
                HirUtilityKind::ReturnType => yelang_ty::ty::UtilityKind::ReturnType,
                HirUtilityKind::Params => yelang_ty::ty::UtilityKind::Parameters,
                HirUtilityKind::Partial => yelang_ty::ty::UtilityKind::Partial,
                HirUtilityKind::Required => yelang_ty::ty::UtilityKind::Required,
            };
            let lowered_args = interner.mk_generic_args(
                &args
                    .iter()
                    .map(|t| GenericArg::Type(lower_hir_ty(t, fcx)))
                    .collect::<Vec<_>>(),
            );
            interner.mk_ty(TyKind::Utility(kind, lowered_args))
        }
        HirTyKind::Ref { mutability, ty } => {
            let mutbl = lower_mutability(mutability.clone());
            let inner = lower_hir_ty(ty, fcx);
            interner.mk_ty(TyKind::Ref(inner, mutbl))
        }
        HirTyKind::RawPtr { mutability, ty } => {
            let mutbl = lower_mutability(mutability.clone());
            let inner = lower_hir_ty(ty, fcx);
            interner.mk_ty(TyKind::RawPtr(TypeAndMut { ty: inner, mutbl }))
        }
        HirTyKind::ForAll { ty, .. } => {
            // HRTB: for now just lower the inner type
            lower_hir_ty(ty, fcx)
        }
        HirTyKind::Union { tys } => {
            if tys.is_empty() {
                return fcx.mk_never();
            }
            let first = lower_hir_ty(&tys[0], fcx);
            tys.iter().skip(1).fold(first, |acc, t| {
                let lowered = lower_hir_ty(t, fcx);
                interner.mk_ty(TyKind::Union(acc, lowered))
            })
        }
        HirTyKind::ImplTrait { path } => {
            if let Res::Def { def_id } = path {
                interner.mk_ty(TyKind::Alias(AliasTy {
                    def_id: *def_id,
                    args: yelang_ty::list::List::empty(),
                }))
            } else {
                fcx.new_ty_var()
            }
        }
        HirTyKind::DynTrait { path } => {
            if let Res::Def { def_id } = path {
                // TODO: proper existential predicate
                let pred = yelang_ty::ty::ExistentialPredicate::Trait(
                    yelang_ty::ty::ExistentialTraitRef {
                        def_id: *def_id,
                        args: yelang_ty::list::List::empty(),
                    },
                );
                interner.mk_ty(TyKind::Dynamic(yelang_ty::ty::Binder {
                    bound_vars: yelang_ty::list::List::empty(),
                    value: pred,
                    _marker: std::marker::PhantomData,
                }))
            } else {
                fcx.new_ty_var()
            }
        }
        HirTyKind::Infer => fcx.new_ty_var(),
        HirTyKind::Err => fcx.mk_error(),
    }
}

fn lower_res<'tcx>(res: &Res, fcx: &mut FnCtxt<'tcx>) -> Ty<'tcx> {
    let interner = fcx.interner;

    match res {
        Res::Def { def_id } => {
            // Look up the item type from the collector
            if let Some(ty) = fcx.item_ty(*def_id) {
                ty
            } else {
                // Fallback: create an ADT type with no args
                interner.mk_ty(TyKind::Adt(
                    AdtDef { def_id: *def_id },
                    yelang_ty::list::List::empty(),
                ))
            }
        }
        Res::Local { .. } => {
            // Local variables shouldn't appear in type position
            fcx.new_ty_var()
        }
        Res::PrimTy { ty } => lower_prim_ty(ty, fcx),
        Res::SelfTy { def_id } => {
            if let Some(ty) = fcx.self_ty {
                ty
            } else {
                interner.mk_ty(TyKind::Adt(
                    AdtDef { def_id: *def_id },
                    yelang_ty::list::List::empty(),
                ))
            }
        }
        Res::SelfVal { def_id } => interner.mk_ty(TyKind::Adt(
            AdtDef { def_id: *def_id },
            yelang_ty::list::List::empty(),
        )),
        Res::Err => fcx.mk_error(),
    }
}

fn lower_prim_ty<'tcx>(prim: &PrimTy, fcx: &mut FnCtxt<'tcx>) -> Ty<'tcx> {
    match prim {
        PrimTy::Int(it) => match it {
            HirIntTy::I8 => fcx.mk_int(IntTy::I8),
            HirIntTy::I16 => fcx.mk_int(IntTy::I16),
            HirIntTy::I32 => fcx.mk_int(IntTy::I32),
            HirIntTy::I64 => fcx.mk_int(IntTy::I64),
            HirIntTy::I128 => fcx.mk_int(IntTy::I128),
            HirIntTy::Isize => fcx.mk_int(IntTy::Isize),
            HirIntTy::U8 => fcx.mk_uint(UintTy::U8),
            HirIntTy::U16 => fcx.mk_uint(UintTy::U16),
            HirIntTy::U32 => fcx.mk_uint(UintTy::U32),
            HirIntTy::U64 => fcx.mk_uint(UintTy::U64),
            HirIntTy::U128 => fcx.mk_uint(UintTy::U128),
            HirIntTy::Usize => fcx.mk_uint(UintTy::Usize),
        },
        PrimTy::Float(ft) => match ft {
            HirFloatTy::F32 => fcx.mk_float(FloatTy::F32),
            HirFloatTy::F64 => fcx.mk_float(FloatTy::F64),
        },
        PrimTy::Bool => fcx.mk_bool(),
        PrimTy::Char => fcx.mk_char(),
        PrimTy::Str => fcx.mk_str(),
    }
}

fn lower_mutability(m: yelang_ast::Mutability) -> Mutability {
    match m {
        yelang_ast::Mutability::Mutable => Mutability::Mut,
        yelang_ast::Mutability::Immutable => Mutability::Not,
    }
}
