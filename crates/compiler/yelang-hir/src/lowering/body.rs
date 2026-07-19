//! Lowering of AST blocks into HIR `Body` values.

use yelang_ast::{BlockExpr, Expr as AstExpr, Param as AstParam};

use crate::hir::body::Body;
use crate::hir::core::Expr;
use crate::ids::BodyId;
use crate::lowering::LoweringContext;

/// Lower a `BlockExpr` into a standalone `Body` and register it in the crate.
///
/// `params` are the AST parameters of the function/trait method/closure. Their
/// patterns are lowered into the current scope so that the body can refer to
/// them by name, and the whole scope is popped when the body is complete so
/// bindings do not leak into sibling items.
pub fn lower_block_as_body(
    ctx: &mut LoweringContext,
    block: &BlockExpr,
    params: &[AstParam],
    param_tys: &[crate::ids::HirTyId],
) -> BodyId {
    ctx.push_scope();

    let params: Vec<crate::hir::body::Param> = params
        .iter()
        .zip(param_tys)
        .map(|(param, &ty)| {
            let pat_id = crate::lowering::pat::lower_pat(ctx, &param.pattern);
            crate::hir::body::Param {
                pat: pat_id,
                ty,
                span: param.span,
            }
        })
        .collect();

    let block = crate::lowering::expr::lower_block(ctx, block);
    let block_span = block.span;
    let value = ctx.crate_hir.alloc_expr(Expr::Block { block }, block_span);

    ctx.pop_scope();

    let body = Body {
        params,
        value,
        span: block_span,
    };

    ctx.crate_hir.alloc_body(body, block_span)
}

/// Lower a single AST expression into a standalone `Body`.
/// Used for const/static initializers and other expression bodies.
pub fn lower_expr_as_body(ctx: &mut LoweringContext, expr: &AstExpr) -> BodyId {
    let value = crate::lowering::expr::lower_expr(ctx, expr);
    let body = Body {
        params: vec![],
        value,
        span: expr.span,
    };
    ctx.crate_hir.alloc_body(body, expr.span)
}
