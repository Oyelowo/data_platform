//! Lowering of AST types to HIR types.

use yelang_ast::Type as AstType;

use crate::hir::core::TraitBound;
use crate::hir::ty::{AnonField, Const, ConstKind, GenericArg, Ty, UtilityKind};
use crate::ids::HirTyId;
use crate::lowering::LoweringContext;
use crate::res::Res;
use yelang_resolve::lang_items::LangItem;

/// Extract and lower generic arguments from an AST path.
pub(crate) fn lower_generic_args_from_path(
    ctx: &mut LoweringContext,
    path: &yelang_ast::Path,
) -> Vec<GenericArg> {
    let mut args = Vec::new();
    for segment in &path.segments {
        let Some(seg_args) = &segment.args else {
            continue;
        };
        match seg_args {
            yelang_ast::GenericArgs::AngleBracketed(ab) => {
                for arg in &ab.args {
                    args.push(lower_generic_arg(ctx, arg));
                }
            }
            // Parenthesized args are function-trait sugar; the trait path itself
            // is resolved, so no extra type args are emitted here.
            yelang_ast::GenericArgs::Parenthesized(_) => {}
        }
    }
    args
}

fn lower_generic_arg(ctx: &mut LoweringContext, arg: &yelang_ast::AngleBracketedArg) -> GenericArg {
    match arg {
        yelang_ast::AngleBracketedArg::Type(ty) => GenericArg::Type(lower_ty(ctx, ty)),
        yelang_ast::AngleBracketedArg::Const(expr) => {
            GenericArg::Const(lower_const_expr(ctx, expr, expr.span))
        }
        yelang_ast::AngleBracketedArg::AssociatedType { name, ty } => GenericArg::AssocBinding {
            name: *name,
            ty: lower_ty(ctx, ty),
        },
    }
}

/// Lower an AST constant expression into a HIR `Const`.
pub(crate) fn lower_const_expr(
    ctx: &mut LoweringContext,
    expr: &yelang_ast::Expr,
    span: yelang_lexer::Span,
) -> Const {
    let body = crate::lowering::body::lower_expr_as_body(ctx, expr);
    Const {
        kind: ConstKind::Expr { body },
        span,
    }
}

/// Lower an AST type to a HIR type, allocate it in the crate arena, and return
/// its `HirTyId`.
pub fn lower_ty(ctx: &mut LoweringContext, ty: &AstType) -> HirTyId {
    let span = ty.span;
    let kind = match &ty.kind {
        yelang_ast::TypeKind::Named(path) => {
            let res = crate::lowering::expr::resolve_ast_path(ctx, path);
            let args = lower_generic_args_from_path(ctx, path);
            Ty::Path { res, args }
        }
        yelang_ast::TypeKind::Tuple(tys) => Ty::Tuple {
            tys: tys.iter().map(|t| lower_ty(ctx, t)).collect(),
        },
        yelang_ast::TypeKind::Array(inner, len) => Ty::Array {
            ty: lower_ty(ctx, inner),
            len: lower_const_expr(ctx, len, len.span),
        },
        yelang_ast::TypeKind::Slice(inner) => {
            // Surface `[T]` is the dynamic array type `Array<T>`.
            let elem_ty = lower_ty(ctx, inner);
            if let Some(def_id) = ctx.resolved.lang_items.get(LangItem::Array) {
                Ty::Path {
                    res: Res::Def { def_id },
                    args: vec![GenericArg::Type(elem_ty)],
                }
            } else {
                // Fallback for isolated tests without a prelude: keep the slice
                // representation so HIR remains well-formed.
                Ty::Slice { ty: elem_ty }
            }
        }
        yelang_ast::TypeKind::Ref { ty: inner, is_mut } => Ty::Ref {
            mutability: if *is_mut {
                yelang_ast::Mutability::Mutable
            } else {
                yelang_ast::Mutability::Immutable
            },
            ty: lower_ty(ctx, inner),
        },
        yelang_ast::TypeKind::RawPtr { ty: inner, is_mut } => Ty::RawPtr {
            mutability: if *is_mut {
                yelang_ast::Mutability::Mutable
            } else {
                yelang_ast::Mutability::Immutable
            },
            ty: lower_ty(ctx, inner),
        },
        yelang_ast::TypeKind::Function(fn_ty) => Ty::FnPtr {
            sig: Box::new(crate::hir::core::FnSig {
                inputs: fn_ty.params.iter().map(|p| lower_ty(ctx, p)).collect(),
                output: lower_ty(ctx, &fn_ty.return_type),
                is_async: fn_ty.is_async,
                is_const: false,
                is_variadic: fn_ty.is_variadic,
                abi: fn_ty.abi.clone(),
                bound_vars: vec![],
            }),
        },
        yelang_ast::TypeKind::ForAll { params, ty: inner } => {
            let hir_params = lower_type_binder_params(ctx, params);
            Ty::ForAll {
                params: hir_params,
                ty: lower_ty(ctx, inner),
            }
        }
        yelang_ast::TypeKind::Literal(lit) => Ty::TypeLit {
            variants: vec![lit.clone()],
        },
        yelang_ast::TypeKind::Structural(fields) => Ty::AnonStruct {
            fields: fields
                .iter()
                .map(|f| AnonField {
                    name: f.name.symbol,
                    ty: lower_ty(ctx, &f.ty),
                })
                .collect(),
        },
        yelang_ast::TypeKind::Union(tys) => Ty::Union {
            tys: tys.iter().map(|t| lower_ty(ctx, t)).collect(),
        },
        yelang_ast::TypeKind::Operator(op) => lower_type_operator(ctx, op, span),
        yelang_ast::TypeKind::ImplTrait(path) => Ty::ImplTrait {
            path: crate::lowering::expr::resolve_ast_path(ctx, path),
        },
        yelang_ast::TypeKind::DynTrait(path) => Ty::DynTrait {
            path: crate::lowering::expr::resolve_ast_path(ctx, path),
        },
        yelang_ast::TypeKind::Infer => Ty::Infer,
        yelang_ast::TypeKind::Never => Ty::Never,
        yelang_ast::TypeKind::Error => Ty::Err,
    };

    ctx.crate_hir.alloc_ty(kind, span)
}

/// Lower `TypeBinderParams` (from `for<T>`) into `BinderParam`s.
///
/// HRTB binders are not item-level generic parameters, so they do not carry
/// `DefId`s and do not support defaults.
pub(crate) fn lower_type_binder_params(
    ctx: &mut LoweringContext,
    params: &yelang_ast::item::TypeBinderParams,
) -> Vec<crate::hir::core::BinderParam> {
    use crate::hir::core::BinderParam;
    params
        .params
        .iter()
        .map(|p| match p {
            yelang_ast::item::TypeBinderParam::Type(tp) => BinderParam::Type {
                name: tp.name,
                bounds: tp
                    .bounds
                    .iter()
                    .map(|b| lower_trait_bound(ctx, b))
                    .collect(),
                span: tp.span,
            },
            yelang_ast::item::TypeBinderParam::Const(cp) => BinderParam::Const {
                name: cp.name,
                ty: lower_ty(ctx, &cp.ty),
                span: cp.span,
            },
        })
        .collect()
}

/// Lower a single trait bound.
pub(crate) fn lower_trait_bound(
    ctx: &mut LoweringContext,
    bound: &yelang_ast::TraitBound,
) -> TraitBound {
    TraitBound {
        path: crate::lowering::expr::resolve_ast_path(ctx, &bound.path),
        args: lower_generic_args_from_path(ctx, &bound.path),
        span: bound.span,
    }
}

/// Lower a `TypeOperator` into a HIR `Ty`.
fn lower_type_operator(
    ctx: &mut LoweringContext,
    op: &yelang_ast::TypeOperator,
    _span: yelang_lexer::Span,
) -> Ty {
    match op {
        yelang_ast::TypeOperator::TypeOf(expr) => {
            // `typeof expr` evaluates to the type of the expression at
            // type-check time. The expression is lowered and stored directly
            // so the type checker can query its type.
            let expr_id = crate::lowering::expr::lower_expr(ctx, expr);
            Ty::TypeOf { expr: expr_id }
        }
        yelang_ast::TypeOperator::ReturnType(ty) => Ty::Utility {
            kind: UtilityKind::ReturnType,
            args: vec![lower_ty(ctx, ty)],
        },
        yelang_ast::TypeOperator::Parameters(ty) => Ty::Utility {
            kind: UtilityKind::Params,
            args: vec![lower_ty(ctx, ty)],
        },
        yelang_ast::TypeOperator::Pick(base, keys) => Ty::Utility {
            kind: UtilityKind::Pick,
            args: vec![lower_ty(ctx, base), lower_ty(ctx, keys)],
        },
        yelang_ast::TypeOperator::Omit(base, keys) => Ty::Utility {
            kind: UtilityKind::Omit,
            args: vec![lower_ty(ctx, base), lower_ty(ctx, keys)],
        },
    }
}
