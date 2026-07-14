//! Lowering of AST types to HIR types.

use yelang_ast::Type as AstType;

use crate::hir::{GenericParam, TraitBound};
use crate::hir_ty::{AnonField, Ty, TyKind, Const, ConstKind, UtilityKind};
use crate::lowering::LoweringContext;

/// Lower an AST type to a HIR type.
pub fn lower_ty(ctx: &mut LoweringContext, ty: &AstType) -> Ty {
    let span = ty.span;
    let kind = match &ty.kind {
        yelang_ast::TypeKind::Named(path) => {
            let res = crate::lowering_expr::resolve_ast_path(ctx, path);
            TyKind::Path { res }
        }
        yelang_ast::TypeKind::Tuple(tys) => TyKind::Tuple {
            tys: tys.iter().map(|t| lower_ty(ctx, t)).collect(),
        },
        yelang_ast::TypeKind::Array(inner, len) => TyKind::Array {
            ty: Box::new(lower_ty(ctx, inner)),
            // TODO: Lower the length expression to a proper Const.
            // For now we emit a placeholder; const-eval will replace it.
            len: Const {
                kind: ConstKind::Lit {
                    lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                        value: yelang_interner::Symbol::from(0u32),
                        suffix: None,
                    }),
                },
                span: len.span,
            },
        },
        yelang_ast::TypeKind::Slice(inner) => TyKind::Slice {
            ty: Box::new(lower_ty(ctx, inner)),
        },
        yelang_ast::TypeKind::Ref { ty: inner, is_mut } => TyKind::Ref {
            mutability: if *is_mut {
                yelang_ast::Mutability::Mutable
            } else {
                yelang_ast::Mutability::Immutable
            },
            ty: Box::new(lower_ty(ctx, inner)),
        },
        yelang_ast::TypeKind::Function(fn_ty) => TyKind::FnPtr {
            sig: Box::new(crate::hir::FnSig {
                inputs: fn_ty
                    .params
                    .iter()
                    .map(|p| lower_ty(ctx, p))
                    .collect(),
                output: lower_ty(ctx, &fn_ty.return_type),
                is_async: false,
                is_const: false,
                is_variadic: false,
                bound_vars: vec![],
            }),
        },
        yelang_ast::TypeKind::ForAll { params, ty: inner } => {
            let hir_params = lower_type_binder_params(ctx, params);
            TyKind::ForAll {
                params: hir_params,
                ty: Box::new(lower_ty(ctx, inner)),
            }
        }
        yelang_ast::TypeKind::Literal(lit) => TyKind::TypeLit {
            variants: vec![lit.clone()],
        },
        yelang_ast::TypeKind::Structural(fields) => TyKind::AnonStruct {
            fields: fields
                .iter()
                .map(|f| AnonField {
                    name: f.name.symbol,
                    ty: lower_ty(ctx, &f.ty),
                })
                .collect(),
        },
        yelang_ast::TypeKind::Union(tys) => TyKind::Union {
            tys: tys.iter().map(|t| lower_ty(ctx, t)).collect(),
        },
        yelang_ast::TypeKind::Operator(op) => lower_type_operator(ctx, op),
        yelang_ast::TypeKind::ImplTrait(path) => TyKind::ImplTrait {
            path: crate::lowering_expr::resolve_ast_path(ctx, path),
        },
        yelang_ast::TypeKind::DynTrait(path) => TyKind::DynTrait {
            path: crate::lowering_expr::resolve_ast_path(ctx, path),
        },
        yelang_ast::TypeKind::Infer => TyKind::Infer,
        yelang_ast::TypeKind::Never => TyKind::Tuple { tys: vec![] },
        yelang_ast::TypeKind::Error => TyKind::Err,
    };

    Ty { kind, span }
}

/// Lower `TypeBinderParams` (from `for<T>`) into item-level `GenericParam`s.
///
/// HRTB binders do not support defaults, so `default` is always `None`.
pub(crate) fn lower_type_binder_params(
    ctx: &mut LoweringContext,
    params: &yelang_ast::item::TypeBinderParams,
) -> Vec<GenericParam> {
    params
        .params
        .iter()
        .map(|p| match p {
            yelang_ast::item::TypeBinderParam::Type(tp) => GenericParam::Type {
                name: tp.name,
                bounds: tp
                    .bounds
                    .iter()
                    .map(|b| lower_trait_bound(ctx, b))
                    .collect(),
                default: None,
                span: tp.span,
            },
            yelang_ast::item::TypeBinderParam::Const(cp) => GenericParam::Const {
                name: cp.name,
                ty: lower_ty(ctx, &cp.ty),
                default: None,
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
        path: crate::lowering_expr::resolve_ast_path(ctx, &bound.path),
        span: bound.span,
    }
}

/// Lower a `TypeOperator` into a `Utility` `TyKind`.
fn lower_type_operator(ctx: &mut LoweringContext, op: &yelang_ast::TypeOperator) -> TyKind {
    match op {
        yelang_ast::TypeOperator::TypeOf(_expr) => {
            // `typeof expr` is an unevaluated type operator.
            // We lower it as a special Utility marker; type-checking will
            // evaluate the expression and substitute the real type.
            TyKind::Utility {
                kind: UtilityKind::ReturnType, // Placeholder; typeof is its own kind
                args: vec![],
            }
        }
        yelang_ast::TypeOperator::ReturnType(ty) => TyKind::Utility {
            kind: UtilityKind::ReturnType,
            args: vec![lower_ty(ctx, ty)],
        },
        yelang_ast::TypeOperator::Parameters(ty) => TyKind::Utility {
            kind: UtilityKind::Params,
            args: vec![lower_ty(ctx, ty)],
        },
        yelang_ast::TypeOperator::Pick(base, keys) => TyKind::Utility {
            kind: UtilityKind::Pick,
            args: vec![lower_ty(ctx, base), lower_ty(ctx, keys)],
        },
        yelang_ast::TypeOperator::Omit(base, keys) => TyKind::Utility {
            kind: UtilityKind::Omit,
            args: vec![lower_ty(ctx, base), lower_ty(ctx, keys)],
        },
    }
}
