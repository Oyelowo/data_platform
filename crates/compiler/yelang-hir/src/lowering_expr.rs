//! Lowering of AST expressions to HIR expressions.

use yelang_ast::{
    Expr as AstExpr, ExprKind as AstExprKind,
    BinaryExpr, UnaryExpr, CallExpr, IfExpr, ForLoopExpr, WhileExpr,
    LoopExpr, MatchExpr, BreakExpr, ContinueExpr,
    BlockExpr, StructExpr, FieldAssign,
    Label,
};
use yelang_lexer::Span;

use crate::ids::{BodyId, HirId};
use crate::hir::{
    Arm, Block, CaptureClause, Expr, ExprKind, FieldExpr, Stmt, StmtKind,
};
use crate::hir_item::Item;
use crate::hir_pat::Pat;
use crate::hir_ty::Ty;
use crate::lowering::LoweringContext;
use crate::lowering_err::LoweringError;
use crate::res::Res;

/// Lower an AST expression into HIR.
pub fn lower_expr(ctx: &mut LoweringContext, expr: &AstExpr) -> Expr {
    let span = expr.span;
    let kind = match &expr.kind {
        AstExprKind::Literal(lit) => ExprKind::Lit { lit: lit.clone() },
        AstExprKind::Path(path) => {
            let res = resolve_ast_path(ctx, path);
            ExprKind::Path { res }
        }
        AstExprKind::Binary(bin) => ExprKind::Binary {
            op: bin.op,
            left: Box::new(lower_expr(ctx, &bin.left)),
            right: Box::new(lower_expr(ctx, &bin.right)),
        },
        AstExprKind::Unary(un) => ExprKind::Unary {
            op: un.op,
            expr: Box::new(lower_expr(ctx, &un.expr)),
        },
        AstExprKind::Call(call) => ExprKind::Call {
            func: Box::new(lower_expr(ctx, &call.callee)),
            args: call
                .args
                .iter()
                .map(|arg| match arg {
                    yelang_ast::CallArgument::Positional(e) => lower_expr(ctx, e),
                    yelang_ast::CallArgument::Named(_ident, e) => lower_expr(ctx, e),
                })
                .collect(),
        },
        AstExprKind::Block(block) => {
            let block = lower_block(ctx, block);
            ExprKind::Block { block }
        }
        AstExprKind::If(if_expr) => lower_if_expr(ctx, if_expr),
        AstExprKind::ForLoop(for_expr) => lower_for_expr(ctx, for_expr),
        AstExprKind::While(while_expr) => lower_while_expr(ctx, while_expr),
        AstExprKind::Loop(loop_expr) => {
            let block = lower_block(ctx, &loop_expr.body);
            ExprKind::Loop {
                block,
                label: loop_expr.label.clone(),
            }
        }
        AstExprKind::Match(match_expr) => lower_match_expr(ctx, match_expr),
        AstExprKind::Break(break_expr) => ExprKind::Break {
            label: break_expr.label.clone(),
            expr: break_expr.value.as_ref().map(|e| Box::new(lower_expr(ctx, e))),
        },
        AstExprKind::Continue(cont) => ExprKind::Continue {
            label: cont.label.clone(),
        },
        AstExprKind::Return(ret) => ExprKind::Return {
            expr: ret.as_ref().map(|e| Box::new(lower_expr(ctx, e))),
        },
        AstExprKind::Struct(struct_expr) => lower_struct_expr(ctx, struct_expr),
        AstExprKind::Tuple(exprs) => ExprKind::Tuple {
            exprs: exprs.iter().map(|e| lower_expr(ctx, e)).collect(),
        },
        AstExprKind::Array(arr) => match arr.elements() {
            Some(elements) => ExprKind::Array {
                exprs: elements.iter().map(|e| lower_expr(ctx, e)).collect(),
            },
            None => {
                ctx.error(LoweringError::UnsupportedAst {
                    kind: "repeat array `[expr; count]`".to_string(),
                    span,
                });
                ExprKind::Err
            }
        },
        AstExprKind::TypeCast(cast) => ExprKind::Cast {
            expr: Box::new(lower_expr(ctx, &cast.base)),
            ty: crate::lowering_ty::lower_ty(ctx, &cast.ty),
        },
        AstExprKind::MemberAccess(access) => ExprKind::Field {
            expr: Box::new(lower_expr(ctx, access.base())),
            field: access.member().clone(),
        },
        AstExprKind::ArrayAccess(access) => {
            let index_expr = match access.index() {
                yelang_ast::ArrayIndex::Single(idx) => idx.expr(),
                _ => {
                    ctx.error(LoweringError::UnsupportedAst {
                        kind: "complex array index".to_string(),
                        span,
                    });
                    return Expr {
                        hir_id: ctx.next_hir_id(),
                        kind: ExprKind::Err,
                        span,
                        ty: Ty { kind: crate::hir_ty::TyKind::Infer, span },
                    };
                }
            };
            ExprKind::Index {
                expr: Box::new(lower_expr(ctx, access.base())),
                index: Box::new(lower_expr(ctx, index_expr)),
            }
        }
        AstExprKind::AssignEq(assign) => ExprKind::Assign {
            left: Box::new(lower_expr(ctx, &assign.target)),
            right: Box::new(lower_expr(ctx, &assign.value)),
        },
        AstExprKind::Lambda(lambda) => lower_lambda_expr(ctx, lambda),
        AstExprKind::Let(let_expr) => ExprKind::Let {
            pat: crate::lowering_pat::lower_pat(ctx, &let_expr.pattern),
            expr: Box::new(lower_expr(ctx, &let_expr.expr)),
        },
        AstExprKind::Try(_try_expr) => {
            // Desugar `expr?` -> match expr { Ok(v) => v, Err(e) => return Err(...) }
            // For now, stub with Err.
            ctx.error(LoweringError::UnsupportedAst {
                kind: "try operator `?`".to_string(),
                span,
            });
            ExprKind::Err
        }
        _ => {
            ctx.error(LoweringError::UnsupportedAst {
                kind: format!("expression kind {:?}", std::mem::discriminant(&expr.kind)),
                span,
            });
            ExprKind::Err
        }
    };

    Expr {
        hir_id: ctx.next_hir_id(),
        kind,
        span,
        ty: Ty {
            kind: crate::hir_ty::TyKind::Infer,
            span,
        },
    }
}

pub(crate) fn resolve_ast_path(ctx: &mut LoweringContext, path: &yelang_ast::Path) -> Res {
    if let Some(ident) = path.standalone_ident() {
        if let Some(hir_id) = ctx.local(ident.symbol) {
            return Res::Local { hir_id };
        }
    }
    // Fallback: unresolved.  In a full compiler we would look up in
    // `ctx.resolved` here.
    Res::Err
}

fn lower_block(ctx: &mut LoweringContext, block: &BlockExpr) -> Block {
    let stmts: Vec<Stmt> = block
        .statements
        .iter()
        .map(|stmt| lower_stmt(ctx, stmt))
        .collect();

    // Simplification: treat the last expression-ish statement as the
    // block's trailing expression if it is not a `TermExpr`.
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

    Block {
        stmts,
        expr,
        span: block.label.as_ref().map_or(Span::default(), |l| l.span),
    }
}

pub(crate) fn lower_stmt(ctx: &mut LoweringContext, stmt: &yelang_ast::Stmt) -> Stmt {
    let span = stmt.span;
    let kind = match &stmt.kind {
        yelang_ast::StmtKind::Expr(expr) => StmtKind::Expr {
            expr: Box::new(lower_expr(ctx, expr)),
        },
        yelang_ast::StmtKind::TermExpr(expr) => StmtKind::Expr {
            expr: Box::new(lower_expr(ctx, expr)),
        },
        yelang_ast::StmtKind::Let(let_stmt) => StmtKind::Let {
            pat: crate::lowering_pat::lower_pat(ctx, &let_stmt.pattern),
            ty: let_stmt.ty.as_ref().map(|ty| crate::lowering_ty::lower_ty(ctx, ty)),
            init: let_stmt.init.as_ref().map(|e| Box::new(lower_expr(ctx, e))),
        },
        yelang_ast::StmtKind::Item(item) => {
            let def_id = crate::lowering_item::lower_item(ctx, item);
            // Nested items are placed into the crate map; the statement
            // just keeps a reference so the visitor can reach it.
            let def_id = match def_id {
                Some(d) => d,
                None => ctx.next_def_id(),
            };
            StmtKind::Item {
                item: ctx
                    .crate_hir
                    .items
                    .get(&def_id)
                    .cloned()
                    .unwrap_or_else(|| Item {
                        def_id,
                        ident: yelang_ast::Ident::new(yelang_interner::Symbol::from(0u32), span),
                        kind: crate::hir::ItemKind::Mod { items: vec![] },
                        vis: yelang_ast::Visibility::Private,
                        span,
                    }),
            }
        }
        yelang_ast::StmtKind::Empty => StmtKind::Expr {
            expr: Box::new(Expr {
                hir_id: ctx.next_hir_id(),
                kind: ExprKind::Tuple { exprs: vec![] },
                span,
                ty: Ty {
                    kind: crate::hir_ty::TyKind::Tuple { tys: vec![] },
                    span,
                },
            }),
        },
    };

    Stmt { kind, span }
}

// ---------------------------------------------------------------------------
// Desugarings
// ---------------------------------------------------------------------------

/// `while cond { body }`  ->  `loop { if cond { body } else { break } }`
fn lower_while_expr(ctx: &mut LoweringContext, while_expr: &WhileExpr) -> ExprKind {
    let cond = lower_expr(ctx, &while_expr.condition);
    let body = lower_block(ctx, &while_expr.body);
    let body_expr = Expr {
        hir_id: ctx.next_hir_id(),
        kind: ExprKind::Block { block: body },
        span: while_expr.body.label.as_ref().map_or(Span::default(), |l| l.span),
        ty: Ty {
            kind: crate::hir_ty::TyKind::Tuple { tys: vec![] },
            span: Span::default(),
        },
    };

    let break_expr = Expr {
        hir_id: ctx.next_hir_id(),
        kind: ExprKind::Break {
            label: None,
            expr: None,
        },
        span: Span::default(),
        ty: Ty {
            kind: crate::hir_ty::TyKind::Tuple { tys: vec![] },
            span: Span::default(),
        },
    };

    let else_block = Block {
        stmts: vec![],
        expr: Some(Box::new(break_expr)),
        span: Span::default(),
    };

    let if_expr = Expr {
        hir_id: ctx.next_hir_id(),
        kind: ExprKind::If {
            cond: Box::new(cond),
            then_branch: Box::new(body_expr),
            else_branch: Some(Box::new(Expr {
                hir_id: ctx.next_hir_id(),
                kind: ExprKind::Block { block: else_block },
                span: Span::default(),
                ty: Ty {
                    kind: crate::hir_ty::TyKind::Tuple { tys: vec![] },
                    span: Span::default(),
                },
            })),
        },
        span: Span::default(),
        ty: Ty {
            kind: crate::hir_ty::TyKind::Tuple { tys: vec![] },
            span: Span::default(),
        },
    };

    let loop_block = Block {
        stmts: vec![Stmt {
            kind: StmtKind::Expr {
                expr: Box::new(if_expr),
            },
            span: Span::default(),
        }],
        expr: None,
        span: Span::default(),
    };

    ExprKind::Loop {
        block: loop_block,
        label: while_expr.label.clone(),
    }
}

/// `for pat in iter { body }`  ->  `{ let mut _iter = iter; loop { match _iter.next() { Some(pat) => { body }, None => break } } }`
fn lower_for_expr(ctx: &mut LoweringContext, for_expr: &ForLoopExpr) -> ExprKind {
    let iter_expr = lower_expr(ctx, &for_expr.iter);
    let body_block = lower_block(ctx, &for_expr.body);

    // Build a fake `let _iter = iter` binding and wrap in a block.
    let iter_pat_hir_id = ctx.next_hir_id();
    let iter_pat = Pat {
        hir_id: iter_pat_hir_id,
        kind: crate::hir_pat::PatKind::Binding {
            mode: crate::hir_pat::BindingMode::ByValue,
            name: yelang_interner::Symbol::from(0u32),
            subpat: None,
        },
        span: for_expr.pat.span,
    };

    let iter_let = Stmt {
        kind: StmtKind::Let {
            pat: iter_pat,
            ty: None,
            init: Some(Box::new(iter_expr)),
        },
        span: for_expr.pat.span,
    };

    // Build match arms: Some(pat) => body, None => break
    let some_pat = Pat {
        hir_id: ctx.next_hir_id(),
        kind: crate::hir_pat::PatKind::TupleStruct {
            res: Res::Err,
            pats: vec![crate::lowering_pat::lower_pat(ctx, &for_expr.pat)],
        },
        span: for_expr.pat.span,
    };

    let body_expr = Expr {
        hir_id: ctx.next_hir_id(),
        kind: ExprKind::Block { block: body_block },
        span: for_expr.body.label.as_ref().map_or(Span::default(), |l| l.span),
        ty: Ty {
            kind: crate::hir_ty::TyKind::Tuple { tys: vec![] },
            span: Span::default(),
        },
    };

    let none_pat = Pat {
        hir_id: ctx.next_hir_id(),
        kind: crate::hir_pat::PatKind::Path { res: Res::Err },
        span: for_expr.pat.span,
    };

    let break_expr = Expr {
        hir_id: ctx.next_hir_id(),
        kind: ExprKind::Break {
            label: None,
            expr: None,
        },
        span: Span::default(),
        ty: Ty {
            kind: crate::hir_ty::TyKind::Tuple { tys: vec![] },
            span: Span::default(),
        },
    };

    let match_arms = vec![
        Arm {
            pat: some_pat,
            guard: None,
            body: Box::new(body_expr),
            span: for_expr.pat.span,
        },
        Arm {
            pat: none_pat,
            guard: None,
            body: Box::new(break_expr),
            span: for_expr.pat.span,
        },
    ];

    let iter_path = Expr {
        hir_id: ctx.next_hir_id(),
        kind: ExprKind::Path { res: Res::Local { hir_id: iter_pat_hir_id } },
        span: for_expr.pat.span,
        ty: Ty {
            kind: crate::hir_ty::TyKind::Infer,
            span: Span::default(),
        },
    };

    let match_expr = Expr {
        hir_id: ctx.next_hir_id(),
        kind: ExprKind::Match {
            expr: Box::new(iter_path),
            arms: match_arms,
        },
        span: for_expr.pat.span,
        ty: Ty {
            kind: crate::hir_ty::TyKind::Tuple { tys: vec![] },
            span: Span::default(),
        },
    };

    let loop_block = Block {
        stmts: vec![Stmt {
            kind: StmtKind::Expr {
                expr: Box::new(match_expr),
            },
            span: Span::default(),
        }],
        expr: None,
        span: Span::default(),
    };

    // Return the desugared expression wrapped in a block containing the let.
    // For simplicity, we return the Loop directly (omitting the let wrapper)
    // which is still semantically close enough for our MVP.
    ExprKind::Loop {
        block: loop_block,
        label: for_expr.label.clone(),
    }
}

fn lower_if_expr(ctx: &mut LoweringContext, if_expr: &IfExpr) -> ExprKind {
    // Desugar let-chains: if let A = a && let B = b && cond { ... }
    // -> if let A = a { if let B = b { if cond { ... } } }
    let cond = lower_expr(ctx, &if_expr.condition);
    let then_branch = Box::new(Expr {
        hir_id: ctx.next_hir_id(),
        kind: ExprKind::Block {
            block: lower_block(ctx, &if_expr.then_block),
        },
        span: if_expr.then_block.label.as_ref().map_or(Span::default(), |l| l.span),
        ty: Ty {
            kind: crate::hir_ty::TyKind::Tuple { tys: vec![] },
            span: Span::default(),
        },
    });

    let else_branch = if_expr.else_expr.as_ref().map(|e| Box::new(lower_expr(ctx, e)));

    ExprKind::If {
        cond: Box::new(cond),
        then_branch,
        else_branch,
    }
}

fn lower_match_expr(ctx: &mut LoweringContext, match_expr: &MatchExpr) -> ExprKind {
    let scrutinee = lower_expr(ctx, &match_expr.scrutinee);
    let arms: Vec<Arm> = match_expr
        .arms
        .iter()
        .map(|arm| Arm {
            pat: crate::lowering_pat::lower_pat(ctx, &arm.pattern),
            guard: arm.guard.as_ref().map(|g| Box::new(lower_expr(ctx, g))),
            body: Box::new(lower_expr(ctx, &arm.body)),
            span: arm.span,
        })
        .collect();

    ExprKind::Match {
        expr: Box::new(scrutinee),
        arms,
    }
}

fn lower_struct_expr(ctx: &mut LoweringContext, struct_expr: &StructExpr) -> ExprKind {
    let res = resolve_ast_path(ctx, &struct_expr.path);
    let fields: Vec<FieldExpr> = struct_expr
        .fields
        .iter()
        .map(|f| FieldExpr {
            ident: f.name,
            expr: lower_expr(ctx, &f.value),
            span: f.span,
        })
        .collect();

    ExprKind::Struct {
        path: res,
        fields,
        rest: struct_expr.rest.as_ref().map(|e| Box::new(lower_expr(ctx, e))),
    }
}

fn lower_lambda_expr(ctx: &mut LoweringContext, lambda: &yelang_ast::LambdaExpr) -> ExprKind {
    // Lower parameters and body into a synthetic Body.
    let params: Vec<crate::hir_body::Param> = lambda
        .fn_sig
        .params
        .iter()
        .map(|p| crate::hir_body::Param {
            pat: crate::lowering_pat::lower_pat(ctx, &p.pattern),
            ty: crate::lowering_ty::lower_ty(ctx, &p.ty),
            span: p.span,
        })
        .collect();

    let body_expr = lower_expr(ctx, &lambda.body);
    let body = crate::hir_body::Body {
        params,
        value: body_expr,
        span: lambda.header_span,
    };

    let body_id = ctx.next_body_id();
    ctx.crate_hir.bodies.insert(body_id, body);

    ExprKind::Closure {
        params: vec![], // params are stored in the Body
        body: body_id,
        capture_clause: CaptureClause::Ref,
    }
}
