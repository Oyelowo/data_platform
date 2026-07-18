//! Lowering of AST blocks into HIR `Body` values.

use yelang_ast::{BlockExpr, Expr as AstExpr};
use yelang_lexer::Span;

use crate::hir::core::Expr;
use crate::hir::body::Body;
use crate::hir::pat::{BindingMode, Pat};
use crate::ids::{BodyId, HirTyId};
use crate::lowering::LoweringContext;

/// Lower a `BlockExpr` into a standalone `Body` and register it in the crate.
pub fn lower_block_as_body(
    ctx: &mut LoweringContext,
    block: &BlockExpr,
    param_tys: &[HirTyId],
) -> BodyId {
    // Build synthetic patterns for each parameter type so we have PatIds.
    let params: Vec<crate::hir::body::Param> = param_tys
        .iter()
        .enumerate()
        .map(|(i, ty)| {
            let name = yelang_interner::Symbol::from(i as u32);
            let _ty_node = ctx
                .crate_hir
                .ty(*ty)
                .expect("parameter type should be allocated");
            let pat_id = ctx.crate_hir.alloc_pat(
                Pat::Binding {
                    mode: BindingMode::ByValue,
                    name,
                    subpat: None,
                },
                ty_node_span(ctx, *ty),
            );
            ctx.push_local(name, pat_id);
            crate::hir::body::Param {
                pat: pat_id,
                ty: *ty,
                span: ty_node_span(ctx, *ty),
            }
        })
        .collect();

    let block = crate::lowering::expr::lower_block(ctx, block);
    let block_span = block.span;
    let value = ctx
        .crate_hir
        .alloc_expr(Expr::Block { block }, block_span);

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

fn ty_node_span(ctx: &LoweringContext, ty: HirTyId) -> Span {
    ctx.crate_hir.ty_spans.get(ty).copied().unwrap_or(Span::default())
}
