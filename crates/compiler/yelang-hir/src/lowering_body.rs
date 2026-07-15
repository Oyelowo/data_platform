//! Lowering of AST blocks into HIR `Body` values.

use yelang_ast::{BlockExpr, Expr as AstExpr};
use yelang_lexer::Span;

use crate::hir::{Block, Expr, ExprKind, Stmt, StmtKind};
use crate::hir_body::Body;
use crate::hir_pat::Pat;
use crate::hir_ty::Ty;
use crate::ids::BodyId;
use crate::lowering::LoweringContext;

/// Lower a `BlockExpr` into a standalone `Body` and register it in the crate.
pub fn lower_block_as_body(
    ctx: &mut LoweringContext,
    block: &BlockExpr,
    param_tys: &[Ty],
) -> BodyId {
    let body_id = ctx.next_body_id();

    // Build synthetic patterns for each parameter type so we have HirIds.
    let params: Vec<crate::hir_body::Param> = param_tys
        .iter()
        .enumerate()
        .map(|(i, ty)| {
            let hir_id = ctx.next_hir_id();
            let name = yelang_interner::Symbol::from(i as u32);
            ctx.push_local(name, hir_id);
            crate::hir_body::Param {
                pat: Pat {
                    hir_id,
                    kind: crate::hir_pat::PatKind::Binding {
                        mode: crate::hir_pat::BindingMode::ByValue,
                        name,
                        subpat: None,
                    },
                    span: ty.span,
                },
                ty: ty.clone(),
                span: ty.span,
            }
        })
        .collect();

    let stmts: Vec<Stmt> = block
        .statements
        .iter()
        .map(|stmt| crate::lowering_expr::lower_stmt(ctx, stmt))
        .collect();

    let (stmts, expr) = if let Some(last) = stmts.last() {
        match &last.kind {
            StmtKind::Expr { expr: e } => {
                let mut stmts = stmts;
                let expr = stmts.pop().map(|s| match s.kind {
                    StmtKind::Expr { expr } => expr,
                    _ => unreachable!(),
                });
                (stmts, expr)
            }
            _ => (stmts, None),
        }
    } else {
        (stmts, None)
    };

    let block_span = block.label.as_ref().map_or(Span::default(), |l| l.span);
    let block = Block {
        stmts,
        expr,
        span: block_span,
    };

    let value = Expr {
        hir_id: ctx.next_hir_id(),
        kind: ExprKind::Block { block },
        span: block_span,
        ty: Ty {
            kind: crate::hir_ty::TyKind::Infer,
            span: block_span,
        },
    };

    let body = Body {
        params,
        value,
        span: block_span,
    };

    ctx.crate_hir.bodies.insert(body_id, body);
    body_id
}

/// Lower a single AST expression into a standalone `Body`.
/// Used for const/static initializers and other expression bodies.
pub fn lower_expr_as_body(ctx: &mut LoweringContext, expr: &AstExpr) -> BodyId {
    let body_id = ctx.next_body_id();
    let value = crate::lowering_expr::lower_expr(ctx, expr);
    let body = Body {
        params: vec![],
        value,
        span: expr.span,
    };
    ctx.crate_hir.bodies.insert(body_id, body);
    body_id
}
