//! Lowering of AST types to HIR types.

use yelang_ast::Type as AstType;
use yelang_lexer::Span;

use crate::hir_ty::{Ty, TyKind, Const, ConstKind};
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
        yelang_ast::TypeKind::Infer => TyKind::Infer,
        yelang_ast::TypeKind::Never => TyKind::Tuple { tys: vec![] },
        yelang_ast::TypeKind::Error => TyKind::Err,
        _ => {
            ctx.error(crate::lowering_err::LoweringError::UnsupportedAst {
                kind: format!("type kind {:?}", std::mem::discriminant(&ty.kind)),
                span,
            });
            TyKind::Err
        }
    };

    Ty { kind, span }
}
