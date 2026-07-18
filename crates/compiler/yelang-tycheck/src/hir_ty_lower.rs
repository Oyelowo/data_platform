/*! Lower HIR types to canonical `yelang_ty::Ty`.
 *
 * HIR types (`hir::Ty`) are syntactic and already have resolved paths.
 * This module converts them to the interned type representation.
 */

use yelang_hir::hir::ty::{GenericArg as HirGenericArg, Ty as HirTy, UtilityKind as HirUtilityKind};
use yelang_hir::res::{FloatTy as HirFloatTy, IntTy as HirIntTy, PrimTy, Res};
use yelang_ty::generic::GenericArg;
use yelang_ty::primitive::{FloatTy, IntTy, UintTy};
use yelang_ty::ty::{
    AdtDef, AliasTy, AnonField, AnonStructDef, Mutability, Ty, TyKind, TypeAndMut,
};

use crate::fn_ctxt::FnCtxt;

/// Lower a HIR type to a canonical type.
pub fn lower_hir_ty<'tcx>(hir_ty: &HirTy, fcx: &mut FnCtxt<'tcx>) -> Ty<'tcx> {
    lower_hir_ty_value(hir_ty, fcx)
}

fn lower_hir_ty_value<'tcx>(ty: &HirTy, fcx: &mut FnCtxt<'tcx>) -> Ty<'tcx> {
    let interner = fcx.interner;

    match ty {
        HirTy::Path { res, args } => lower_res(res, args, fcx),
        HirTy::Tuple { tys } => {
            let lowered: Vec<_> = tys
                .iter()
                .map(|t| lower_hir_ty_id(*t, fcx))
                .collect();
            let args = interner.mk_generic_args(
                &lowered
                    .iter()
                    .map(|&t| GenericArg::Type(t))
                    .collect::<Vec<_>>(),
            );
            interner.mk_ty(TyKind::Tuple(args))
        }
        HirTy::Array { ty, len } => {
            let elem_ty = lower_hir_ty_id(*ty, fcx);
            let len_const = lower_hir_const(len, elem_ty, fcx);
            interner.mk_ty(TyKind::Array(elem_ty, len_const))
        }
        HirTy::Slice { ty } => {
            let elem_ty = lower_hir_ty_id(*ty, fcx);
            interner.mk_ty(TyKind::Slice(elem_ty))
        }
        HirTy::FnPtr { sig } => {
            let inputs = interner.mk_generic_args(
                &sig.inputs
                    .iter()
                    .map(|t| GenericArg::Type(lower_hir_ty_id(*t, fcx)))
                    .collect::<Vec<_>>(),
            );
            let output = lower_hir_ty_id(sig.output, fcx);
            interner.mk_ty(TyKind::FnPtr(yelang_ty::ty::PolyFnSig {
                sig: yelang_ty::ty::FnSig { inputs, output },
            }))
        }
        HirTy::AnonStruct { fields } => {
            let lowered_fields: Vec<_> = fields
                .iter()
                .map(|f| AnonField {
                    name: f.name,
                    ty: lower_hir_ty_id(f.ty, fcx),
                })
                .collect();
            let field_list = yelang_ty::list::List::from_slice(&lowered_fields);
            interner.mk_ty(TyKind::AnonStruct(AnonStructDef {
                fields: field_list,
            }))
        }
        HirTy::TypeLit { .. } => {
            // Type literals are union-like; for now return a fresh variable
            fcx.new_ty_var()
        }
        HirTy::Utility { kind, args } => {
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
                    .map(|t| GenericArg::Type(lower_hir_ty_id(*t, fcx)))
                    .collect::<Vec<_>>(),
            );
            interner.mk_ty(TyKind::Utility(kind, lowered_args))
        }
        HirTy::Ref { mutability, ty } => {
            let mutbl = lower_mutability(mutability.clone());
            let inner = lower_hir_ty_id(*ty, fcx);
            interner.mk_ty(TyKind::Ref(inner, mutbl))
        }
        HirTy::RawPtr { mutability, ty } => {
            let mutbl = lower_mutability(mutability.clone());
            let inner = lower_hir_ty_id(*ty, fcx);
            interner.mk_ty(TyKind::RawPtr(TypeAndMut { ty: inner, mutbl }))
        }
        HirTy::ForAll { ty, .. } => {
            // HRTB: for now just lower the inner type
            lower_hir_ty_id(*ty, fcx)
        }
        HirTy::Union { tys } => {
            if tys.is_empty() {
                return fcx.mk_never();
            }
            let first = lower_hir_ty_id(tys[0], fcx);
            tys.iter().skip(1).fold(first, |acc, t| {
                let lowered = lower_hir_ty_id(*t, fcx);
                interner.mk_ty(TyKind::Union(acc, lowered))
            })
        }
        HirTy::TypeOf { expr } => {
            // `typeof expr` evaluates to the type of the expression.
            let ty = crate::check::check_expr(fcx, *expr);
            ty
        }
        HirTy::Never => fcx.mk_never(),
        HirTy::Missing => fcx.new_ty_var(),
        HirTy::ImplTrait { path } => {
            if let Res::Def { def_id } = path {
                interner.mk_ty(TyKind::Alias(AliasTy {
                    def_id: *def_id,
                    args: yelang_ty::list::List::empty(),
                }))
            } else {
                fcx.new_ty_var()
            }
        }
        HirTy::DynTrait { path } => {
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
        HirTy::Infer => fcx.new_ty_var(),
        HirTy::Err => fcx.mk_error(),
    }
}

fn lower_hir_const<'tcx>(
    konst: &yelang_hir::hir::ty::Const,
    ty: Ty<'tcx>,
    _fcx: &mut FnCtxt<'tcx>,
) -> yelang_ty::ty::Const<'tcx> {
    use yelang_hir::hir::ty::ConstKind as HirConstKind;
    use yelang_ty::ty::{ConstKind, ConstValue};

    let kind = match &konst.kind {
        HirConstKind::Lit { lit } => match lit {
            yelang_lexer::Literal::Int(il) => {
                // Parse the integer symbol as an i128 (best effort).
                let s = il.value.to_string();
                if let Ok(v) = s.parse::<i128>() {
                    ConstKind::Value(ConstValue::Int(v))
                } else {
                    ConstKind::Error
                }
            }
            yelang_lexer::Literal::Float(fl) => {
                let s = fl.value.to_string();
                if let Ok(v) = s.parse::<f64>() {
                    ConstKind::Value(ConstValue::Float(v))
                } else {
                    ConstKind::Error
                }
            }
            yelang_lexer::Literal::Str(sl) => ConstKind::Value(ConstValue::Str(sl.value)),
            yelang_lexer::Literal::Char(cl) => ConstKind::Value(ConstValue::Int(*cl as i128)),
            yelang_lexer::Literal::Bool(b) => ConstKind::Value(ConstValue::Bool(*b)),
            // Non-scalar literals cannot appear in const-generic positions.
            yelang_lexer::Literal::Regex(_)
            | yelang_lexer::Literal::DateTime(_)
            | yelang_lexer::Literal::Duration(_)
            | yelang_lexer::Literal::Bytes(_)
            | yelang_lexer::Literal::Uuid(_)
            | yelang_lexer::Literal::Geometry(_)
            | yelang_lexer::Literal::RecordId(_)
            | yelang_lexer::Literal::Unit => ConstKind::Error,
        },
        HirConstKind::Expr { body: _ } => {
            // TODO: const-eval the body once the const evaluator is available.
            // For now leave the length/dimension as an error constant so that
            // type checking does not crash.
            ConstKind::Error
        }
        HirConstKind::Err => ConstKind::Error,
    };

    yelang_ty::ty::Const { kind, ty }
}

fn lower_hir_ty_id<'tcx>(ty_id: yelang_hir::ids::TyId, fcx: &mut FnCtxt<'tcx>) -> Ty<'tcx> {
    let hir_ty = fcx
        .crate_hir
        .tys
        .get(ty_id)
        .expect("TyId should be valid")
        .clone();
    lower_hir_ty(&hir_ty, fcx)
}

fn lower_res<'tcx>(res: &Res, args: &[HirGenericArg], fcx: &mut FnCtxt<'tcx>) -> Ty<'tcx> {
    let interner = fcx.interner;
    let lowered_args = lower_generic_args(args, fcx);

    match res {
        Res::Def { def_id } => {
            // Look up the item type from the collector
            if let Some(ty) = fcx.item_ty(*def_id) {
                ty
            } else {
                // Fallback: create an ADT type with the lowered generic args.
                interner.mk_ty(TyKind::Adt(AdtDef { def_id: *def_id }, lowered_args))
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
                interner.mk_ty(TyKind::Adt(AdtDef { def_id: *def_id }, lowered_args))
            }
        }
        Res::SelfVal { def_id } => {
            interner.mk_ty(TyKind::Adt(AdtDef { def_id: *def_id }, lowered_args))
        }
        Res::Err => fcx.mk_error(),
    }
}

fn lower_generic_args<'tcx>(
    args: &[HirGenericArg],
    fcx: &mut FnCtxt<'tcx>,
) -> yelang_ty::list::List<GenericArg<'tcx>> {
    let interner = fcx.interner;
    if args.is_empty() {
        return yelang_ty::list::List::empty();
    }
    interner.mk_generic_args(
        &args
            .iter()
            .filter_map(|arg| match arg {
                HirGenericArg::Type(ty_id) => Some(GenericArg::Type(lower_hir_ty_id(*ty_id, fcx))),
                HirGenericArg::Const(_) => {
                    // TODO: lower const generic arguments once the type system
                    // supports them. For now treat them as absent.
                    None
                }
                HirGenericArg::AssocBinding { .. } => {
                    // TODO: lower associated type bindings once the type system
                    // supports them. For now treat them as absent.
                    None
                }
            })
            .collect::<Vec<_>>(),
    )
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
