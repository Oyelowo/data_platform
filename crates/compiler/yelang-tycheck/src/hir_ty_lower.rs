/*! Lower HIR types to canonical `yelang_ty::TyId`.
 *
 * HIR types (`hir::HirTy`) are syntactic and already have resolved paths.
 * This module converts them to the interned type representation.
 *
 * The lowering is parameterized over a [`TyLowerCtxt`] so that both the
 * signature collector and the body type checker can reuse the same logic.
 */

use yelang_hir::hir::ty::{GenericArg as HirGenericArg, HirTy, UtilityKind as HirUtilityKind};
use yelang_hir::ids::HirTyId;
use yelang_hir::res::{FloatTy as HirFloatTy, IntTy as HirIntTy, PrimTy, Res};
use yelang_ty::generic::GenericArg;

use yelang_ty::primitive::{FloatTy, IntTy, UintTy};
use yelang_ty::ty::{
    AdtDef, AliasTy, AnonField, AnonStructDef, ConstId, Mutability, Ty, TyId, TypeAndMut,
};

use crate::lower_ctx::TyLowerCtxt;

/// Lower a HIR type to a canonical type.
pub fn lower_hir_ty<Cx: TyLowerCtxt>(hir_ty: &HirTy, cx: &mut Cx) -> TyId {
    lower_hir_ty_value(hir_ty, cx)
}

/// Lower a HIR type node by ID.
pub fn lower_hir_ty_id<Cx: TyLowerCtxt>(ty_id: HirTyId, cx: &mut Cx) -> TyId {
    let hir_ty = cx
        .crate_hir()
        .ty(ty_id)
        .expect("HirTyId should be valid")
        .clone();
    lower_hir_ty(&hir_ty, cx)
}

fn lower_hir_ty_value<Cx: TyLowerCtxt>(ty: &HirTy, cx: &mut Cx) -> TyId {
    match ty {
        HirTy::Path { res, args } => lower_res(res, args, cx),
        HirTy::Tuple { tys } => {
            let lowered: Vec<_> = tys
                .iter()
                .map(|t| lower_hir_ty_id(*t, cx))
                .collect();
            let args = cx.interner().mk_generic_args(
                &lowered
                    .iter()
                    .map(|&t| GenericArg::Type(t))
                    .collect::<Vec<_>>(),
            );
            cx.mk_ty(Ty::Tuple(args))
        }
        HirTy::Array { ty, len } => {
            let elem_ty = lower_hir_ty_id(*ty, cx);
            let len_const = lower_hir_const(len, elem_ty, cx);
            cx.mk_ty(Ty::Array(elem_ty, len_const))
        }
        HirTy::Slice { ty } => {
            let elem_ty = lower_hir_ty_id(*ty, cx);
            cx.mk_ty(Ty::Slice(elem_ty))
        }
        HirTy::FnPtr { sig } => {
            let lowered_inputs: Vec<_> = sig.inputs
                .iter()
                .map(|t| GenericArg::Type(lower_hir_ty_id(*t, cx)))
                .collect();
            let inputs = cx.interner().mk_generic_args(&lowered_inputs);
            let output = lower_hir_ty_id(sig.output, cx);
            cx.mk_ty(Ty::FnPtr(yelang_ty::ty::PolyFnSig {
                sig: yelang_ty::ty::FnSig { inputs, output },
            }))
        }
        HirTy::AnonStruct { fields } => {
            let lowered_fields: Vec<_> = fields
                .iter()
                .map(|f| AnonField {
                    name: f.name,
                    ty: lower_hir_ty_id(f.ty, cx),
                })
                .collect();
            let field_list = yelang_ty::list::List::from_slice(&lowered_fields);
            cx.mk_ty(Ty::AnonStruct(AnonStructDef {
                fields: field_list,
            }))
        }
        HirTy::TypeLit { .. } => {
            // Type literals are union-like; for now return a fresh variable
            cx.lower_infer()
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
            let lowered_args: Vec<_> = args
                .iter()
                .map(|t| GenericArg::Type(lower_hir_ty_id(*t, cx)))
                .collect();
            let lowered_args = cx.interner().mk_generic_args(&lowered_args);
            cx.mk_ty(Ty::Utility(kind, lowered_args))
        }
        HirTy::Ref { mutability, ty } => {
            let mutbl = lower_mutability(mutability.clone());
            let inner = lower_hir_ty_id(*ty, cx);
            cx.mk_ty(Ty::Ref(inner, mutbl))
        }
        HirTy::RawPtr { mutability, ty } => {
            let mutbl = lower_mutability(mutability.clone());
            let inner = lower_hir_ty_id(*ty, cx);
            cx.mk_ty(Ty::RawPtr(TypeAndMut { ty: inner, mutbl }))
        }
        HirTy::ForAll { ty, .. } => {
            // HRTB: for now just lower the inner type
            lower_hir_ty_id(*ty, cx)
        }
        HirTy::Union { tys } => {
            if tys.is_empty() {
                return cx.mk_never();
            }
            let first = lower_hir_ty_id(tys[0], cx);
            tys.iter().skip(1).fold(first, |acc, t| {
                let lowered = lower_hir_ty_id(*t, cx);
                cx.mk_ty(Ty::Union(acc, lowered))
            })
        }
        HirTy::TypeOf { expr } => cx.lower_typeof(*expr),
        HirTy::Never => cx.mk_never(),
        HirTy::Missing => cx.lower_missing(),
        HirTy::ImplTrait { path } => {
            if let Res::Def { def_id } = path {
                cx.mk_ty(Ty::Alias(AliasTy {
                    def_id: *def_id,
                    args: yelang_ty::list::List::empty(),
                }))
            } else {
                cx.lower_infer()
            }
        }
        HirTy::DynTrait { path } => {
            if let Res::Def { def_id } = path {
                // TODO: proper existential predicate list
                let pred = yelang_ty::ty::ExistentialPredicate::Trait(
                    yelang_ty::ty::ExistentialTraitRef {
                        def_id: *def_id,
                        args: yelang_ty::list::List::empty(),
                    },
                );
                let preds = cx.interner().mk_existential_predicates(&[pred]);
                cx.mk_ty(Ty::Dynamic(yelang_ty::ty::Binder {
                    bound_vars: yelang_ty::list::List::empty(),
                    value: preds,
                }))
            } else {
                cx.lower_infer()
            }
        }
        HirTy::Infer => cx.lower_infer(),
        HirTy::Err => cx.mk_error(),
    }
}

fn lower_hir_const<Cx: TyLowerCtxt>(
    konst: &yelang_hir::hir::ty::Const,
    ty: TyId,
    cx: &mut Cx,
) -> ConstId {
    use yelang_hir::hir::ty::ConstKind as HirConstKind;
    use yelang_ty::ty::{Const, ConstValue};

    let kind = match &konst.kind {
        HirConstKind::Lit { lit } => match lit {
            yelang_lexer::Literal::Int(il) => {
                // Parse the integer symbol as an i128 (best effort).
                let s = il.value.to_string();
                if let Ok(v) = s.parse::<i128>() {
                    Const::Value(ConstValue::Int(v))
                } else {
                    Const::Error
                }
            }
            yelang_lexer::Literal::Float(fl) => {
                let s = fl.value.to_string();
                if let Ok(v) = s.parse::<f64>() {
                    Const::Value(ConstValue::Float(v))
                } else {
                    Const::Error
                }
            }
            yelang_lexer::Literal::Str(sl) => Const::Value(ConstValue::Str(sl.value)),
            yelang_lexer::Literal::Char(cl) => Const::Value(ConstValue::Int(*cl as i128)),
            yelang_lexer::Literal::Bool(b) => Const::Value(ConstValue::Bool(*b)),
            // Non-scalar literals cannot appear in const-generic positions.
            yelang_lexer::Literal::Regex(_)
            | yelang_lexer::Literal::DateTime(_)
            | yelang_lexer::Literal::Duration(_)
            | yelang_lexer::Literal::Bytes(_)
            | yelang_lexer::Literal::Uuid(_)
            | yelang_lexer::Literal::Geometry(_)
            | yelang_lexer::Literal::RecordId(_)
            | yelang_lexer::Literal::Unit => Const::Error,
        },
        HirConstKind::Expr { body: _ } => {
            // TODO: const-eval the body once the const evaluator is available.
            // For now leave the length/dimension as an error constant so that
            // type checking does not crash.
            Const::Error
        }
        HirConstKind::Err => Const::Error,
    };

    cx.interner().mk_const_from_parts(kind, ty)
}

fn lower_res<Cx: TyLowerCtxt>(res: &Res, args: &[HirGenericArg], cx: &mut Cx) -> TyId {
    let lowered_args = lower_generic_args(args, cx);

    match res {
        Res::Def { def_id } => {
            // Type parameters are resolved to DefIds too; check those first.
            if let Some(ty) = cx.param_ty(*def_id) {
                ty
            } else if let Some(ty) = cx.item_ty(*def_id) {
                ty
            } else {
                // Fallback: create an ADT type with the lowered generic args.
                cx.mk_ty(Ty::Adt(AdtDef { def_id: *def_id }, lowered_args))
            }
        }
        Res::Local { .. } => {
            // Local variables shouldn't appear in type position
            cx.lower_infer()
        }
        Res::PrimTy { ty } => lower_prim_ty(ty, cx),
        Res::SelfTy { def_id } => {
            if let Some(ty) = cx.self_ty() {
                ty
            } else {
                cx.mk_ty(Ty::Adt(AdtDef { def_id: *def_id }, lowered_args))
            }
        }
        Res::SelfVal { def_id } => {
            cx.mk_ty(Ty::Adt(AdtDef { def_id: *def_id }, lowered_args))
        }
        Res::Err => cx.mk_error(),
    }
}

fn lower_generic_args<Cx: TyLowerCtxt>(
    args: &[HirGenericArg],
    cx: &mut Cx,
) -> yelang_ty::list::List<GenericArg> {
    if args.is_empty() {
        return yelang_ty::list::List::empty();
    }
    let lowered: Vec<_> = args
        .iter()
        .filter_map(|arg| match arg {
            HirGenericArg::Type(ty_id) => Some(GenericArg::Type(lower_hir_ty_id(*ty_id, cx))),
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
        .collect();
    cx.interner().mk_generic_args(&lowered)
}

fn lower_prim_ty<Cx: TyLowerCtxt>(prim: &PrimTy, cx: &mut Cx) -> TyId {
    match prim {
        PrimTy::Int(it) => match it {
            HirIntTy::I8 => cx.mk_int(IntTy::I8),
            HirIntTy::I16 => cx.mk_int(IntTy::I16),
            HirIntTy::I32 => cx.mk_int(IntTy::I32),
            HirIntTy::I64 => cx.mk_int(IntTy::I64),
            HirIntTy::I128 => cx.mk_int(IntTy::I128),
            HirIntTy::Isize => cx.mk_int(IntTy::Isize),
            HirIntTy::U8 => cx.mk_uint(UintTy::U8),
            HirIntTy::U16 => cx.mk_uint(UintTy::U16),
            HirIntTy::U32 => cx.mk_uint(UintTy::U32),
            HirIntTy::U64 => cx.mk_uint(UintTy::U64),
            HirIntTy::U128 => cx.mk_uint(UintTy::U128),
            HirIntTy::Usize => cx.mk_uint(UintTy::Usize),
        },
        PrimTy::Float(ft) => match ft {
            HirFloatTy::F32 => cx.mk_float(FloatTy::F32),
            HirFloatTy::F64 => cx.mk_float(FloatTy::F64),
        },
        PrimTy::Bool => cx.mk_bool(),
        PrimTy::Char => cx.mk_char(),
        PrimTy::Str => cx.mk_str(),
    }
}

fn lower_mutability(m: yelang_ast::Mutability) -> Mutability {
    match m {
        yelang_ast::Mutability::Mutable => Mutability::Mut,
        yelang_ast::Mutability::Immutable => Mutability::Not,
    }
}
