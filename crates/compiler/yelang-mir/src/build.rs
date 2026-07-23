//! THIR → MIR lowering (MIR building).
//!
//! Converts THIR bodies into MIR bodies using a builder pattern.
//! Each THIR expression becomes a sequence of MIR statements + terminators.
//! Control flow (if, match, loop) creates new basic blocks.
//!
//! Query expressions (`ThirExpr::Query`) are NOT lowered to MIR.
//! They go through QIR. The QIR↔MIR bridge converts query results
//! into MIR locals (see `bridge.rs`).

use yelang_lexer::Span;
use yelang_thir::{ThirExpr, ThirExprId, ThirPat, ThirPatId, ThirStmt, ThirStmtId};
use yelang_ty::ty::TyId;

use crate::body::*;
use crate::ops::*;
use crate::place::*;
use crate::terminator::*;

/// The MIR builder: appends statements and terminators to basic blocks.
pub struct MirBuilder {
    /// The body being built.
    pub body: Body,
    /// The current basic block being appended to.
    current_block: BasicBlock,
    /// The THIR expression arena (for looking up expressions).
    thir_exprs: slotmap::SlotMap<ThirExprId, ThirExpr>,
    /// The THIR statement arena.
    thir_stmts: slotmap::SlotMap<ThirStmtId, ThirStmt>,
    /// The THIR pattern arena.
    _thir_pats: slotmap::SlotMap<ThirPatId, ThirPat>,
    /// The type interner.
    interner: yelang_ty::interner::Interner,
}

impl MirBuilder {
    /// Create a new MIR builder for a function body.
    pub fn new(
        arg_count: usize,
        return_ty: TyId,
        span: Span,
        thir_exprs: slotmap::SlotMap<ThirExprId, ThirExpr>,
        thir_stmts: slotmap::SlotMap<ThirStmtId, ThirStmt>,
        thir_pats: slotmap::SlotMap<ThirPatId, ThirPat>,
        interner: yelang_ty::interner::Interner,
    ) -> Self {
        let body = Body::new(arg_count, return_ty, span);
        let current_block = body.entry_block();
        Self {
            body,
            current_block,
            thir_exprs,
            thir_stmts,
            _thir_pats: thir_pats,
            interner,
        }
    }

    /// Allocate a new temporary local and return it.
    pub fn new_temp(&mut self, ty: TyId) -> Local {
        self.body.new_temp(ty)
    }

    /// Allocate a new basic block and return it.
    pub fn new_block(&mut self) -> BasicBlock {
        self.body.new_block()
    }

    /// Switch to a different basic block.
    pub fn switch_to_block(&mut self, block: BasicBlock) {
        self.current_block = block;
    }

    /// Push a statement to the current block.
    pub fn push_statement(&mut self, stmt: Statement) {
        self.body.basic_blocks[self.current_block]
            .statements
            .push(stmt);
    }

    /// Set the terminator of the current block.
    pub fn set_terminator(&mut self, kind: TerminatorKind, span: Span) {
        self.body.basic_blocks[self.current_block].terminator = Terminator { kind, span };
    }

    /// Emit an assignment: `place = rvalue`.
    pub fn emit_assign(&mut self, place: Place, rvalue: Rvalue) {
        self.push_statement(Statement::Assign(place, rvalue));
    }

    /// Lower a THIR expression and return the Place holding the result.
    ///
    /// This is the core of MIR building. Each THIR expression variant
    /// is converted to MIR statements + a result Place.
    pub fn lower_expr(&mut self, expr_id: ThirExprId, span: Span) -> Place {
        let Some(expr) = self.thir_exprs.get(expr_id).cloned() else {
            // Error recovery: return a dummy temp.
            let ty = self.interner.mk_ty(yelang_ty::ty::Ty::Never);
            let temp = self.new_temp(ty);
            self.set_terminator(TerminatorKind::Unreachable, span);
            return Place::local(temp);
        };

        match &expr {
            ThirExpr::Literal(lit) => {
                let ty = self.interner.mk_ty(yelang_ty::ty::Ty::Never); // TODO: infer from lit
                let temp = self.new_temp(ty);
                let const_val = self.lower_literal(lit);
                self.emit_assign(
                    Place::local(temp),
                    Rvalue::Use(Operand::Constant(Constant {
                        ty,
                        value: const_val,
                    })),
                );
                Place::local(temp)
            }

            ThirExpr::Var(_def_id) => {
                // A variable reference: look up the local for this DefId.
                // For now, create a temp (proper mapping comes from lowering context).
                let ty = self.interner.mk_ty(yelang_ty::ty::Ty::Never); // TODO: resolve type
                let temp = self.new_temp(ty);
                // TODO: emit a copy/move from the actual local
                Place::local(temp)
            }

            ThirExpr::Local(_pat_id) => {
                // A local variable reference.
                let ty = self.interner.mk_ty(yelang_ty::ty::Ty::Never); // TODO: resolve type
                let temp = self.new_temp(ty);
                Place::local(temp)
            }

            ThirExpr::Binary { op, left, right } => {
                let left_place = self.lower_expr(*left, span);
                let right_place = self.lower_expr(*right, span);
                let ty = self.interner.mk_ty(yelang_ty::ty::Ty::Never); // TODO: infer result type
                let temp = self.new_temp(ty);
                let bin_op = self.lower_bin_op(op);
                self.emit_assign(
                    Place::local(temp),
                    Rvalue::BinaryOp(bin_op, Operand::Move(left_place), Operand::Move(right_place)),
                );
                Place::local(temp)
            }

            ThirExpr::Unary { op, expr } => {
                let operand_place = self.lower_expr(*expr, span);
                let ty = self.interner.mk_ty(yelang_ty::ty::Ty::Never); // TODO: infer
                let temp = self.new_temp(ty);
                let un_op = match op {
                    yelang_ast::UnaryOp::Not => UnOp::Not,
                    yelang_ast::UnaryOp::Neg => UnOp::Neg,
                    yelang_ast::UnaryOp::Deref => {
                        // Deref is a place operation, not an rvalue.
                        return operand_place.deref();
                    }
                    _ => UnOp::Not, // fallback
                };
                self.emit_assign(
                    Place::local(temp),
                    Rvalue::UnaryOp(un_op, Operand::Move(operand_place)),
                );
                Place::local(temp)
            }

            ThirExpr::Field { base, field } => {
                let base_place = self.lower_expr(*base, span);
                let ty = self.interner.mk_ty(yelang_ty::ty::Ty::Never); // TODO: resolve field type
                base_place.field(*field, ty)
            }

            ThirExpr::Index { base, index } => {
                let base_place = self.lower_expr(*base, span);
                let index_place = self.lower_expr(*index, span);
                // The index must be a local.
                let index_local = if index_place.is_simple() {
                    index_place.local
                } else {
                    let ty = self.interner.mk_ty(yelang_ty::ty::Ty::Never);
                    let temp = self.new_temp(ty);
                    self.emit_assign(Place::local(temp), Rvalue::Use(Operand::Move(index_place)));
                    temp
                };
                base_place.index(index_local)
            }

            ThirExpr::Call { func, args } => {
                let func_place = self.lower_expr(*func, span);
                let arg_places: Vec<Operand> = args
                    .iter()
                    .map(|&arg| {
                        let place = self.lower_expr(arg, span);
                        Operand::Move(place)
                    })
                    .collect();
                let ty = self.interner.mk_ty(yelang_ty::ty::Ty::Never); // TODO: infer return type
                let temp = self.new_temp(ty);
                let target = self.new_block();
                self.set_terminator(
                    TerminatorKind::Call {
                        func: Operand::Move(func_place),
                        args: arg_places,
                        destination: Place::local(temp),
                        target,
                    },
                    span,
                );
                self.switch_to_block(target);
                Place::local(temp)
            }

            ThirExpr::Block { stmts, tail } => {
                // Lower statements in order.
                for &stmt_id in stmts {
                    self.lower_stmt(stmt_id, span);
                }
                // Lower the tail expression (if any).
                if let Some(tail_id) = tail {
                    self.lower_expr(*tail_id, span)
                } else {
                    // Unit value.
                    let ty = self.interner.mk_ty(yelang_ty::ty::Ty::Never);
                    let temp = self.new_temp(ty);
                    self.emit_assign(
                        Place::local(temp),
                        Rvalue::Aggregate(AggregateKind::Tuple, vec![]),
                    );
                    Place::local(temp)
                }
            }

            ThirExpr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let cond_place = self.lower_expr(*cond, span);
                let then_block = self.new_block();
                let else_block = self.new_block();
                let merge_block = self.new_block();

                // Switch on the condition.
                self.set_terminator(
                    TerminatorKind::SwitchInt {
                        discr: Operand::Move(cond_place),
                        targets: SwitchTargets {
                            branches: vec![(1, then_block)], // true → then
                            otherwise: else_block,           // false → else
                        },
                    },
                    span,
                );

                // Lower then branch.
                self.switch_to_block(then_block);
                let then_result = self.lower_body_expr(*then_branch, span);
                let _then_end = self.current_block;
                self.set_terminator(TerminatorKind::Goto { target: merge_block }, span);

                // Lower else branch.
                self.switch_to_block(else_block);
                let _else_result = if let Some(else_id) = else_branch {
                    self.lower_body_expr(*else_id, span)
                } else {
                    let ty = self.interner.mk_ty(yelang_ty::ty::Ty::Never);
                    let temp = self.new_temp(ty);
                    self.emit_assign(
                        Place::local(temp),
                        Rvalue::Aggregate(AggregateKind::Tuple, vec![]),
                    );
                    Place::local(temp)
                };
                self.set_terminator(TerminatorKind::Goto { target: merge_block }, span);

                // Merge: the result is in a phi node (simplified: use then_result).
                self.switch_to_block(merge_block);
                then_result // TODO: proper phi node / merge
            }

            ThirExpr::Assign { left, right } => {
                let left_place = self.lower_expr(*left, span);
                let right_place = self.lower_expr(*right, span);
                self.emit_assign(left_place, Rvalue::Use(Operand::Move(right_place)));
                // Assignment returns unit.
                let ty = self.interner.mk_ty(yelang_ty::ty::Ty::Never);
                let temp = self.new_temp(ty);
                self.emit_assign(
                    Place::local(temp),
                    Rvalue::Aggregate(AggregateKind::Tuple, vec![]),
                );
                Place::local(temp)
            }

            ThirExpr::Return { expr } => {
                let return_place = Place::local(self.body.return_local());
                if let Some(expr_id) = expr {
                    let value_place = self.lower_expr(*expr_id, span);
                    self.emit_assign(return_place, Rvalue::Use(Operand::Move(value_place)));
                }
                self.set_terminator(TerminatorKind::Return, span);
                // Unreachable after return.
                let ty = self.interner.mk_ty(yelang_ty::ty::Ty::Never);
                let temp = self.new_temp(ty);
                let unreachable_block = self.new_block();
                self.switch_to_block(unreachable_block);
                Place::local(temp)
            }

            ThirExpr::Struct { path, fields, rest: _ } => {
                let field_operands: Vec<Operand> = fields
                    .iter()
                    .map(|&(_, expr_id)| {
                        let place = self.lower_expr(expr_id, span);
                        Operand::Move(place)
                    })
                    .collect();
                // Extract the DefId from the resolution result.
                let def_id = match path {
                    yelang_hir::res::Res::Def { def_id } => *def_id,
                    yelang_hir::res::Res::SelfTy { def_id } => *def_id,
                    yelang_hir::res::Res::SelfVal { def_id } => *def_id,
                    _ => {
                        // Error recovery: use a dummy temp.
                        let ty = self.interner.mk_ty(yelang_ty::ty::Ty::Never);
                        let temp = self.new_temp(ty);
                        self.set_terminator(TerminatorKind::Unreachable, span);
                        return Place::local(temp);
                    }
                };
                let ty = self.interner.mk_ty(yelang_ty::ty::Ty::Never); // TODO: resolve struct type
                let temp = self.new_temp(ty);
                self.emit_assign(
                    Place::local(temp),
                    Rvalue::Aggregate(AggregateKind::Struct(def_id), field_operands),
                );
                Place::local(temp)
            }

            ThirExpr::Tuple { fields } => {
                let field_operands: Vec<Operand> = fields
                    .iter()
                    .map(|&expr_id| {
                        let place = self.lower_expr(expr_id, span);
                        Operand::Move(place)
                    })
                    .collect();
                let ty = self.interner.mk_ty(yelang_ty::ty::Ty::Never);
                let temp = self.new_temp(ty);
                self.emit_assign(
                    Place::local(temp),
                    Rvalue::Aggregate(AggregateKind::Tuple, field_operands),
                );
                Place::local(temp)
            }

            ThirExpr::Array { exprs } => {
                let elem_operands: Vec<Operand> = exprs
                    .iter()
                    .map(|&expr_id| {
                        let place = self.lower_expr(expr_id, span);
                        Operand::Move(place)
                    })
                    .collect();
                let ty = self.interner.mk_ty(yelang_ty::ty::Ty::Never);
                let temp = self.new_temp(ty);
                self.emit_assign(
                    Place::local(temp),
                    Rvalue::Aggregate(AggregateKind::Array(ty), elem_operands),
                );
                Place::local(temp)
            }

            ThirExpr::Cast { expr, ty } => {
                let operand_place = self.lower_expr(*expr, span);
                let mir_ty = ty.0; // ThirTyId wraps TyId
                let temp = self.new_temp(mir_ty);
                self.emit_assign(
                    Place::local(temp),
                    Rvalue::Cast(Operand::Move(operand_place), mir_ty),
                );
                Place::local(temp)
            }

            ThirExpr::Ref { expr, .. } => {
                let place = self.lower_expr(*expr, span);
                let ty = self.interner.mk_ty(yelang_ty::ty::Ty::Never); // TODO: ref type
                let temp = self.new_temp(ty);
                self.emit_assign(Place::local(temp), Rvalue::Ref(place));
                Place::local(temp)
            }

            ThirExpr::Deref { expr } => {
                let place = self.lower_expr(*expr, span);
                place.deref()
            }

            ThirExpr::Loop { body, .. } => {
                let loop_block = self.new_block();
                let exit_block = self.new_block();

                self.set_terminator(TerminatorKind::Goto { target: loop_block }, span);
                self.switch_to_block(loop_block);

                // Lower the loop body.
                let _body_result = self.lower_body_expr(*body, span);

                // Loop back.
                self.set_terminator(TerminatorKind::Goto { target: loop_block }, span);

                // Exit block (reached via break).
                self.switch_to_block(exit_block);
                let ty = self.interner.mk_ty(yelang_ty::ty::Ty::Never);
                let temp = self.new_temp(ty);
                Place::local(temp)
            }

            ThirExpr::Break { expr, .. } => {
                if let Some(expr_id) = expr {
                    let value_place = self.lower_expr(*expr_id, span);
                    let return_place = Place::local(self.body.return_local());
                    self.emit_assign(return_place, Rvalue::Use(Operand::Move(value_place)));
                }
                // TODO: jump to the loop's exit block (need loop context).
                self.set_terminator(TerminatorKind::Unreachable, span);
                let ty = self.interner.mk_ty(yelang_ty::ty::Ty::Never);
                let temp = self.new_temp(ty);
                let unreachable_block = self.new_block();
                self.switch_to_block(unreachable_block);
                Place::local(temp)
            }

            ThirExpr::Continue { .. } => {
                // TODO: jump to the loop's header block (need loop context).
                self.set_terminator(TerminatorKind::Unreachable, span);
                let ty = self.interner.mk_ty(yelang_ty::ty::Ty::Never);
                let temp = self.new_temp(ty);
                let unreachable_block = self.new_block();
                self.switch_to_block(unreachable_block);
                Place::local(temp)
            }

            // Query expressions go through QIR, not MIR.
            ThirExpr::Query(_) => {
                // The QIR↔MIR bridge handles this.
                // For now, create a placeholder temp.
                let ty = self.interner.mk_ty(yelang_ty::ty::Ty::Never);
                let temp = self.new_temp(ty);
                // TODO: call the QIR bridge to get the query result.
                Place::local(temp)
            }

            ThirExpr::Intrinsic { name: _, args: _ } => {
                // Lower intrinsics as function calls.
                let ty = self.interner.mk_ty(yelang_ty::ty::Ty::Never);
                let temp = self.new_temp(ty);
                // TODO: lower intrinsic args and emit call.
                Place::local(temp)
            }

            // Match expressions.
            ThirExpr::Match { scrutinee, arms } => {
                let _scrutinee_place = self.lower_expr(*scrutinee, span);
                let merge_block = self.new_block();
                let ty = self.interner.mk_ty(yelang_ty::ty::Ty::Never);
                let result_temp = self.new_temp(ty);

                // For each arm, create a block and lower the body.
                // Simplified: just lower the first arm's body.
                if let Some(first_arm) = arms.first() {
                    let arm_block = self.new_block();
                    self.set_terminator(TerminatorKind::Goto { target: arm_block }, span);
                    self.switch_to_block(arm_block);
                    let arm_result = self.lower_body_expr(first_arm.body, span);
                    self.emit_assign(
                        Place::local(result_temp),
                        Rvalue::Use(Operand::Move(arm_result)),
                    );
                    self.set_terminator(TerminatorKind::Goto { target: merge_block }, span);
                }

                self.switch_to_block(merge_block);
                Place::local(result_temp)
            }

            // Fallback for unhandled expressions.
            _ => {
                let ty = self.interner.mk_ty(yelang_ty::ty::Ty::Never);
                let temp = self.new_temp(ty);
                Place::local(temp)
            }
        }
    }

    /// Lower a THIR body expression (a ThirBodyId).
    fn lower_body_expr(&mut self, _body_id: yelang_thir::ThirBodyId, _span: Span) -> Place {
        // TODO: look up the body in the THIR bodies and lower its value expression.
        let ty = self.interner.mk_ty(yelang_ty::ty::Ty::Never);
        let temp = self.new_temp(ty);
        Place::local(temp)
    }

    /// Lower a THIR statement.
    fn lower_stmt(&mut self, stmt_id: ThirStmtId, span: Span) {
        let Some(stmt) = self.thir_stmts.get(stmt_id).cloned() else {
            return;
        };

        match &stmt {
            ThirStmt::Let { pat: _, init, .. } => {
                if let Some(init_id) = init {
                    let value_place = self.lower_expr(*init_id, span);
                    // TODO: bind the pattern to the value.
                    // For now, just lower the init expression.
                    let _ = value_place;
                }
            }
            ThirStmt::Expr { expr, .. } => {
                self.lower_expr(*expr, span);
            }
            ThirStmt::Item { .. } => {
                // Nested item declarations are handled at the module level.
                // Nothing to emit in MIR.
            }
        }
    }

    /// Lower a literal to a ConstValue.
    fn lower_literal(&self, lit: &yelang_hir::hir::core::Lit) -> ConstValue {
        match lit {
            yelang_hir::hir::core::Lit::Int(int_lit) => {
                // TODO: resolve int_lit.value Symbol to i128 via string interner
                ConstValue::Int(int_lit.value.as_usize() as i128)
            }
            yelang_hir::hir::core::Lit::Float(float_lit) => {
                // TODO: resolve float_lit.value Symbol to f64 via string interner
                let _ = float_lit;
                ConstValue::Float(0.0)
            }
            yelang_hir::hir::core::Lit::Bool(val) => ConstValue::Bool(*val),
            yelang_hir::hir::core::Lit::Char(val) => ConstValue::Char(*val),
            yelang_hir::hir::core::Lit::Str(str_lit) => ConstValue::Str(str_lit.value),
            _ => ConstValue::Unit,
        }
    }

    /// Lower a binary operator.
    fn lower_bin_op(&self, op: &yelang_ast::BinaryOp) -> BinOp {
        match op {
            yelang_ast::BinaryOp::Add => BinOp::Add,
            yelang_ast::BinaryOp::Subtract => BinOp::Sub,
            yelang_ast::BinaryOp::Multiply => BinOp::Mul,
            yelang_ast::BinaryOp::Divide => BinOp::Div,
            yelang_ast::BinaryOp::Modulo => BinOp::Rem,
            yelang_ast::BinaryOp::BitAnd => BinOp::BitAnd,
            yelang_ast::BinaryOp::BitOr => BinOp::BitOr,
            yelang_ast::BinaryOp::BitXor => BinOp::BitXor,
            yelang_ast::BinaryOp::Shl => BinOp::Shl,
            yelang_ast::BinaryOp::Shr => BinOp::Shr,
            yelang_ast::BinaryOp::Eq => BinOp::Eq,
            yelang_ast::BinaryOp::Ne => BinOp::Ne,
            yelang_ast::BinaryOp::Lt => BinOp::Lt,
            yelang_ast::BinaryOp::Lte => BinOp::Le,
            yelang_ast::BinaryOp::Gt => BinOp::Gt,
            yelang_ast::BinaryOp::Gte => BinOp::Ge,
            _ => BinOp::Add, // fallback
        }
    }

    /// Finish building and return the MIR body.
    pub fn finish(self) -> Body {
        self.body
    }
}
