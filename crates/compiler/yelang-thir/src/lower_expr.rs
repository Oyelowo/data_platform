//! Expression lowering: HIR `Expr` → THIR `ThirExpr`.

use yelang_hir::hir::core::{Arm, Block};
use yelang_hir::ids::{BodyId, ExprId};
use yelang_hir::res::Res;
use yelang_lexer::Span;

use crate::context::LoweringContext;
use crate::errors::LoweringError;
use crate::expr::{ThirArm, ThirExpr};
use crate::ids::{ThirBodyId, ThirExprId};

impl<'a> LoweringContext<'a> {
    /// Lower a HIR body to a THIR body.
    pub fn lower_body(&mut self, body_id: BodyId) -> Result<ThirBodyId, LoweringError> {
        let body = self.hir.body(body_id).ok_or_else(|| LoweringError::Unsupported {
            message: format!("missing HIR body {:?}", body_id),
            span: Span::default(),
        })?;

        let mut params = Vec::new();
        for param in &body.params {
            params.push(self.lower_pat(param.pat));
        }

        let value = self.lower_expr(body.value)?;
        Ok(self.alloc_body(params, value))
    }

    /// Lower a HIR expression to a THIR expression.
    pub fn lower_expr(&mut self, expr_id: ExprId) -> Result<ThirExprId, LoweringError> {
        let span = self.hir.expr_span(expr_id);
        let Some(expr) = self.hir.expr(expr_id) else {
            return Ok(self.alloc_expr(ThirExpr::Err));
        };

        let thir_expr = match expr {
            yelang_hir::hir::expr::Expr::Lit { lit } => ThirExpr::Literal(lit.clone()),

            yelang_hir::hir::expr::Expr::Path { res } => self.lower_path(res, span),

            yelang_hir::hir::expr::Expr::Binary { op, left, right } => ThirExpr::Binary {
                op: op.clone(),
                left: self.lower_expr(*left)?,
                right: self.lower_expr(*right)?,
            },

            yelang_hir::hir::expr::Expr::Unary { op, expr } => ThirExpr::Unary {
                op: op.clone(),
                expr: self.lower_expr(*expr)?,
            },

            yelang_hir::hir::expr::Expr::Call { func, args } => ThirExpr::Call {
                func: self.lower_expr(*func)?,
                args: self.lower_exprs(args)?,
            },

            yelang_hir::hir::expr::Expr::MethodCall {
                receiver,
                method: _,
                args,
                trait_def_id: _,
            } => {
                let resolution = self.typeck_results.method_resolution(expr_id);
                let method_def_id = resolution.and_then(|r| r.method_def_id);
                let Some(def_id) = method_def_id else {
                    return Err(LoweringError::UnresolvedMethodCall { span });
                };

                let mut lowered_args = vec![self.lower_expr(*receiver)?];
                for &arg in args {
                    lowered_args.push(self.lower_expr(arg)?);
                }

                ThirExpr::Call {
                    func: self.alloc_expr(ThirExpr::Var(def_id)),
                    args: lowered_args,
                }
            }

            yelang_hir::hir::expr::Expr::Field { expr, field } => ThirExpr::Field {
                base: self.lower_expr(*expr)?,
                field: field.symbol,
            },

            yelang_hir::hir::expr::Expr::Index { expr, index } => ThirExpr::Index {
                base: self.lower_expr(*expr)?,
                index: self.lower_expr(*index)?,
            },

            yelang_hir::hir::expr::Expr::Assign { left, right } => ThirExpr::Assign {
                left: self.lower_expr(*left)?,
                right: self.lower_expr(*right)?,
            },

            yelang_hir::hir::expr::Expr::AssignOp { op, left, right } => ThirExpr::AssignOp {
                op: op.clone(),
                left: self.lower_expr(*left)?,
                right: self.lower_expr(*right)?,
            },

            yelang_hir::hir::expr::Expr::Block { block } => return self.lower_block(block),

            yelang_hir::hir::expr::Expr::Loop { block, label } => {
                let body = self.lower_loop_body(block)?;
                ThirExpr::Loop {
                    body,
                    label: label.clone(),
                }
            }

            yelang_hir::hir::expr::Expr::Break { label, expr } => ThirExpr::Break {
                label: label.clone(),
                expr: self.lower_opt_expr(*expr)?,
            },

            yelang_hir::hir::expr::Expr::Continue { label } => ThirExpr::Continue {
                label: label.clone(),
            },

            yelang_hir::hir::expr::Expr::Return { expr } => ThirExpr::Return {
                expr: self.lower_opt_expr(*expr)?,
            },

            yelang_hir::hir::expr::Expr::Match { expr, arms } => ThirExpr::Match {
                scrutinee: self.lower_expr(*expr)?,
                arms: arms
                    .iter()
                    .map(|arm| self.lower_arm(arm))
                    .collect::<Result<_, _>>()?,
            },

            yelang_hir::hir::expr::Expr::If {
                cond,
                then_branch,
                else_branch,
            } => ThirExpr::If {
                cond: self.lower_expr(*cond)?,
                then_branch: self.lower_expr_body(*then_branch)?,
                else_branch: self.lower_opt_body(*else_branch)?,
            },

            yelang_hir::hir::expr::Expr::Closure {
                params: _,
                body,
                capture_clause: _,
            } => {
                let body_id = self.lower_body(*body)?;
                let params = self
                    .bodies
                    .bodies
                    .get(body_id)
                    .map(|b| b.params.clone())
                    .unwrap_or_default();
                ThirExpr::Closure { params, body: body_id }
            }

            yelang_hir::hir::expr::Expr::Struct { path, fields, rest } => ThirExpr::Struct {
                path: *path,
                fields: fields
                    .iter()
                    .map(|f| Ok((f.ident.symbol, self.lower_expr(f.expr)?)))
                    .collect::<Result<_, _>>()?,
                rest: self.lower_opt_expr(*rest)?,
            },

            yelang_hir::hir::expr::Expr::Tuple { exprs } => ThirExpr::Tuple {
                fields: self.lower_exprs(exprs)?,
            },

            yelang_hir::hir::expr::Expr::Array { exprs } => ThirExpr::Array {
                exprs: self.lower_exprs(exprs)?,
            },

            yelang_hir::hir::expr::Expr::ArrayRepeat { value, count } => ThirExpr::ArrayRepeat {
                value: self.lower_expr(*value)?,
                count: self.lower_expr(*count)?,
            },

            yelang_hir::hir::expr::Expr::Object { fields } => ThirExpr::Object {
                fields: fields
                    .iter()
                    .map(|f| Ok((f.ident.symbol, self.lower_expr(f.expr)?)))
                    .collect::<Result<_, _>>()?,
            },

            yelang_hir::hir::expr::Expr::Range {
                start,
                end,
                inclusive,
            } => ThirExpr::Range {
                start: self.lower_opt_expr(*start)?,
                end: self.lower_opt_expr(*end)?,
                inclusive: *inclusive,
            },

            yelang_hir::hir::expr::Expr::Cast { expr, ty: _ } => {
                let ty = self.expr_ty(expr_id).ok_or_else(|| LoweringError::Unsupported {
                    message: "missing type for cast".to_string(),
                    span,
                })?;
                ThirExpr::Cast {
                    expr: self.lower_expr(*expr)?,
                    ty,
                }
            }

            yelang_hir::hir::expr::Expr::TypeAscription { expr, ty: _ } => {
                let ty = self.expr_ty(expr_id).ok_or_else(|| LoweringError::Unsupported {
                    message: "missing type for type ascription".to_string(),
                    span,
                })?;
                ThirExpr::TypeAscription {
                    expr: self.lower_expr(*expr)?,
                    ty,
                }
            }

            yelang_hir::hir::expr::Expr::Try { expr } => ThirExpr::Try {
                expr: self.lower_expr(*expr)?,
            },

            yelang_hir::hir::expr::Expr::Await { expr } => ThirExpr::Await {
                expr: self.lower_expr(*expr)?,
            },

            yelang_hir::hir::expr::Expr::Query(query_id) => ThirExpr::Query(*query_id),

            yelang_hir::hir::expr::Expr::Intrinsic { name, args } => ThirExpr::Intrinsic {
                name: name.symbol,
                args: self.lower_exprs(args)?,
            },

            // Forms that HIR does not currently emit or that Phase 1 does not
            // need to represent are lowered to the error node. They do not fail
            // lowering so that partial THIR can still be inspected.
            _ => ThirExpr::Err,
        };

        Ok(self.alloc_expr(thir_expr))
    }

    pub(crate) fn lower_path(&self, res: &Res, _span: Span) -> ThirExpr {
        match res {
            Res::Def { def_id } | Res::SelfVal { def_id } => ThirExpr::Var(*def_id),
            Res::Local { pat_id } => match self.local_pats.get(pat_id) {
                Some(&thir_pat_id) => ThirExpr::Local(thir_pat_id),
                None => ThirExpr::Err,
            },
            _ => ThirExpr::Err,
        }
    }

    pub(crate) fn lower_exprs(&mut self, exprs: &[ExprId]) -> Result<Vec<ThirExprId>, LoweringError> {
        exprs.iter().map(|&e| self.lower_expr(e)).collect()
    }

    pub(crate) fn lower_opt_expr(&mut self, expr: Option<ExprId>) -> Result<Option<ThirExprId>, LoweringError> {
        expr.map(|e| self.lower_expr(e)).transpose()
    }

    pub(crate) fn lower_block(&mut self, block: &Block) -> Result<ThirExprId, LoweringError> {
        let stmts = block
            .stmts
            .iter()
            .map(|&s| self.lower_stmt(s))
            .collect::<Result<_, _>>()?;
        let tail = self.lower_opt_expr(block.expr)?;
        Ok(self.alloc_expr(ThirExpr::Block { stmts, tail }))
    }

    pub(crate) fn lower_loop_body(&mut self, block: &Block) -> Result<ThirBodyId, LoweringError> {
        let value = self.lower_block(block)?;
        Ok(self.alloc_body(vec![], value))
    }

    pub(crate) fn lower_expr_body(&mut self, expr_id: ExprId) -> Result<ThirBodyId, LoweringError> {
        let value = self.lower_expr(expr_id)?;
        Ok(self.alloc_body(vec![], value))
    }

    pub(crate) fn lower_opt_body(&mut self, expr: Option<ExprId>) -> Result<Option<ThirBodyId>, LoweringError> {
        expr.map(|e| self.lower_expr_body(e)).transpose()
    }

    pub(crate) fn lower_arm(&mut self, arm: &Arm) -> Result<ThirArm, LoweringError> {
        let saved = self.local_pats.clone();
        let pat = self.lower_pat(arm.pat);
        let guard = self.lower_opt_expr(arm.guard)?;
        let body = self.lower_expr_body(arm.body)?;
        self.local_pats = saved;
        Ok(ThirArm { pat, guard, body })
    }
}
