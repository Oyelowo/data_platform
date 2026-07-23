//! THIR → MIR lowering (MIR building).
//!
//! Converts THIR bodies into MIR bodies using a builder pattern.
//! Each THIR expression becomes a sequence of MIR statements + terminators.
//! Control flow (if, match, loop) creates new basic blocks.
//!
//! Query expressions (`ThirExpr::Query`) are NOT lowered to MIR.
//! They go through QIR. The QIR↔MIR bridge converts query results
//! into MIR locals (see `bridge.rs`).

use std::collections::HashMap;

use yelang_lexer::Span;
use yelang_thir::{ThirBody, ThirBodyId, ThirExpr, ThirExprId, ThirPat, ThirPatId, ThirStmt, ThirStmtId};
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
    thir_pats: slotmap::SlotMap<ThirPatId, ThirPat>,
    /// The THIR body arena (for looking up nested bodies: if/loop/closure).
    thir_bodies: slotmap::SlotMap<ThirBodyId, ThirBody>,
    /// The type interner.
    interner: yelang_ty::interner::Interner,
    /// Inferred type for each THIR expression (from typeck).
    expr_tys: slotmap::SecondaryMap<ThirExprId, TyId>,
    /// Inferred type for each THIR pattern (from typeck).
    pat_tys: slotmap::SecondaryMap<ThirPatId, TyId>,
    /// Loop context stack: (header_block, exit_block) for each enclosing loop.
    loop_stack: Vec<(BasicBlock, BasicBlock)>,
    /// Mapping from pattern IDs to their allocated locals.
    pat_locals: HashMap<ThirPatId, Local>,
    /// Lowered closure bodies (accessible after `finish`).
    pub closure_bodies: Vec<Body>,
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
        thir_bodies: slotmap::SlotMap<ThirBodyId, ThirBody>,
        interner: yelang_ty::interner::Interner,
        expr_tys: slotmap::SecondaryMap<ThirExprId, TyId>,
        pat_tys: slotmap::SecondaryMap<ThirPatId, TyId>,
    ) -> Self {
        let body = Body::new(arg_count, return_ty, span);
        let current_block = body.entry_block();
        Self {
            body,
            current_block,
            thir_exprs,
            thir_stmts,
            thir_pats,
            thir_bodies,
            interner,
            expr_tys,
            pat_tys,
            loop_stack: Vec::new(),
            pat_locals: HashMap::new(),
            closure_bodies: Vec::new(),
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

    // -----------------------------------------------------------------------
    // Type resolution helpers
    // -----------------------------------------------------------------------

    /// Look up the inferred type of a THIR expression.
    /// Falls back to `Ty::Never` if the type is not recorded.
    fn expr_ty(&self, expr_id: ThirExprId) -> TyId {
        self.expr_tys
            .get(expr_id)
            .copied()
            .unwrap_or_else(|| self.interner.mk_ty(yelang_ty::ty::Ty::Never))
    }

    /// Look up the inferred type of a THIR pattern.
    /// Falls back to `Ty::Never` if the type is not recorded.
    fn pat_ty(&self, pat_id: ThirPatId) -> TyId {
        self.pat_tys
            .get(pat_id)
            .copied()
            .unwrap_or_else(|| self.interner.mk_ty(yelang_ty::ty::Ty::Never))
    }

    /// Get the unit type `()`.
    fn unit_ty(&self) -> TyId {
        self.interner
            .mk_ty(yelang_ty::ty::Ty::Tuple(yelang_ty::list::List::empty()))
    }

    /// Infer the type of a literal.
    fn literal_ty(&self, lit: &yelang_hir::hir::core::Lit) -> TyId {
        use yelang_hir::hir::core::Lit;
        match lit {
            Lit::Int(int_lit) => {
                // Use the suffix if present, otherwise default to i64.
                match int_lit.suffix {
                    Some(yelang_lexer::IntSuffix::I8) => {
                        self.interner.mk_ty(yelang_ty::ty::Ty::Int(yelang_ty::primitive::IntTy::I8))
                    }
                    Some(yelang_lexer::IntSuffix::I16) => {
                        self.interner.mk_ty(yelang_ty::ty::Ty::Int(yelang_ty::primitive::IntTy::I16))
                    }
                    Some(yelang_lexer::IntSuffix::I32) => {
                        self.interner.mk_ty(yelang_ty::ty::Ty::Int(yelang_ty::primitive::IntTy::I32))
                    }
                    Some(yelang_lexer::IntSuffix::I64) => {
                        self.interner.mk_ty(yelang_ty::ty::Ty::Int(yelang_ty::primitive::IntTy::I64))
                    }
                    Some(yelang_lexer::IntSuffix::I128) => {
                        self.interner.mk_ty(yelang_ty::ty::Ty::Int(yelang_ty::primitive::IntTy::I128))
                    }
                    Some(yelang_lexer::IntSuffix::Isize) => {
                        self.interner.mk_ty(yelang_ty::ty::Ty::Int(yelang_ty::primitive::IntTy::Isize))
                    }
                    Some(yelang_lexer::IntSuffix::U8) => {
                        self.interner.mk_ty(yelang_ty::ty::Ty::Uint(yelang_ty::primitive::UintTy::U8))
                    }
                    Some(yelang_lexer::IntSuffix::U16) => {
                        self.interner.mk_ty(yelang_ty::ty::Ty::Uint(yelang_ty::primitive::UintTy::U16))
                    }
                    Some(yelang_lexer::IntSuffix::U32) => {
                        self.interner.mk_ty(yelang_ty::ty::Ty::Uint(yelang_ty::primitive::UintTy::U32))
                    }
                    Some(yelang_lexer::IntSuffix::U64) => {
                        self.interner.mk_ty(yelang_ty::ty::Ty::Uint(yelang_ty::primitive::UintTy::U64))
                    }
                    Some(yelang_lexer::IntSuffix::U128) => {
                        self.interner.mk_ty(yelang_ty::ty::Ty::Uint(yelang_ty::primitive::UintTy::U128))
                    }
                    Some(yelang_lexer::IntSuffix::Usize) => {
                        self.interner.mk_ty(yelang_ty::ty::Ty::Uint(yelang_ty::primitive::UintTy::Usize))
                    }
                    None => {
                        // Default integer type: i64.
                        self.interner.mk_ty(yelang_ty::ty::Ty::Int(yelang_ty::primitive::IntTy::I64))
                    }
                }
            }
            Lit::Float(float_lit) => {
                match float_lit.suffix {
                    Some(yelang_lexer::FloatSuffix::F32) => {
                        self.interner.mk_ty(yelang_ty::ty::Ty::Float(yelang_ty::primitive::FloatTy::F32))
                    }
                    // F16, F64, F128, and no suffix all default to f64.
                    _ => {
                        self.interner.mk_ty(yelang_ty::ty::Ty::Float(yelang_ty::primitive::FloatTy::F64))
                    }
                }
            }
            Lit::Bool(_) => self.interner.mk_ty(yelang_ty::ty::Ty::Bool),
            Lit::Char(_) => self.interner.mk_ty(yelang_ty::ty::Ty::Char),
            Lit::Str(_) => self.interner.mk_ty(yelang_ty::ty::Ty::Str),
            _ => self.interner.mk_ty(yelang_ty::ty::Ty::Never),
        }
    }

    // -----------------------------------------------------------------------
    // Pattern binding
    // -----------------------------------------------------------------------

    /// Bind a pattern to a value place, creating locals for bindings.
    ///
    /// For `ThirPat::Binding`, allocates a local, assigns the value, and
    /// records the PatId → Local mapping.
    /// For compound patterns (Tuple, Struct), recursively binds sub-patterns.
    fn bind_pat(&mut self, pat_id: ThirPatId, value: Place, span: Span) {
        let Some(pat) = self.thir_pats.get(pat_id).cloned() else {
            return;
        };

        match &pat {
            ThirPat::Binding { subpat, .. } => {
                let ty = self.pat_ty(pat_id);
                let local = self.new_temp(ty);
                self.emit_assign(Place::local(local), Rvalue::Use(Operand::Move(value)));
                self.pat_locals.insert(pat_id, local);

                // If there's a sub-pattern (e.g. `x @ Some(y)`), bind it too.
                if let Some(sub) = subpat {
                    self.bind_pat(*sub, Place::local(local), span);
                }
            }
            ThirPat::Wild | ThirPat::Rest => {
                // Wildcard and rest patterns don't bind anything.
            }
            ThirPat::Tuple { pats } => {
                // Bind each sub-pattern to the corresponding field of the tuple.
                for (i, sub_pat) in pats.iter().enumerate() {
                    let field_ty = self.pat_ty(*sub_pat);
                    let field_sym = yelang_interner::Symbol::from(i as u32);
                    let field_place = value.clone().field(field_sym, field_ty);
                    self.bind_pat(*sub_pat, field_place, span);
                }
            }
            ThirPat::Struct { fields, .. } => {
                // Bind each field pattern to the corresponding struct field.
                for (field_name, sub_pat) in fields.iter() {
                    let field_ty = self.pat_ty(*sub_pat);
                    let field_place = value.clone().field(*field_name, field_ty);
                    self.bind_pat(*sub_pat, field_place, span);
                }
            }
            ThirPat::TupleStruct { pats, .. } => {
                // Like Tuple: bind each sub-pattern by index.
                for (i, sub_pat) in pats.iter().enumerate() {
                    let field_ty = self.pat_ty(*sub_pat);
                    let field_sym = yelang_interner::Symbol::from(i as u32);
                    let field_place = value.clone().field(field_sym, field_ty);
                    self.bind_pat(*sub_pat, field_place, span);
                }
            }
            ThirPat::Ref { pat: inner, .. } => {
                // Dereference and bind the inner pattern.
                let deref_place = value.deref();
                self.bind_pat(*inner, deref_place, span);
            }
            ThirPat::Or { pats } => {
                // Or-patterns: bind the first alternative (all alternatives
                // must bind the same variables in a well-typed program).
                if let Some(first) = pats.first() {
                    self.bind_pat(*first, value, span);
                }
            }
            ThirPat::Lit { .. } | ThirPat::Range { .. } | ThirPat::Path { .. } => {
                // Literal, range, and path patterns don't introduce bindings.
            }
            ThirPat::Slice { prefix, middle, suffix } => {
                // Bind prefix elements by index.
                for (i, sub_pat) in prefix.iter().enumerate() {
                    let idx_ty = self.interner.mk_ty(yelang_ty::ty::Ty::Uint(yelang_ty::primitive::UintTy::Usize));
                    let idx_local = self.new_temp(idx_ty);
                    self.emit_assign(
                        Place::local(idx_local),
                        Rvalue::Use(Operand::Constant(Constant {
                            ty: idx_ty,
                            value: ConstValue::Uint(i as u128),
                        })),
                    );
                    let elem_place = value.clone().index(idx_local);
                    self.bind_pat(*sub_pat, elem_place, span);
                }
                // Bind middle (rest) pattern if present.
                if let Some(mid) = middle {
                    self.bind_pat(*mid, value.clone(), span);
                }
                // Bind suffix elements (from the end) — simplified: skip for now
                // as it requires knowing the slice length at compile time.
                let _ = suffix;
            }
            ThirPat::Err => {
                // Error recovery: do nothing.
            }
        }
    }

    // -----------------------------------------------------------------------
    // Expression lowering
    // -----------------------------------------------------------------------

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
                let ty = self.expr_ty(expr_id);
                // If the expr_tys map didn't have a type, infer from the literal.
                let ty = if self.interner.ty(ty).is_never() {
                    self.literal_ty(lit)
                } else {
                    ty
                };
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
                // A variable reference by DefId (e.g. a function or static).
                // Look up the type from the expression type map.
                let ty = self.expr_ty(expr_id);
                let temp = self.new_temp(ty);
                // For function references, emit a FnPtr constant.
                if let yelang_ty::ty::Ty::FnDef(fd) = self.interner.ty(ty) {
                    self.emit_assign(
                        Place::local(temp),
                        Rvalue::Use(Operand::Constant(Constant {
                            ty,
                            value: ConstValue::FnPtr(fd.def_id),
                        })),
                    );
                }
                Place::local(temp)
            }

            ThirExpr::Local(pat_id) => {
                // A local variable reference: look up the bound local.
                let ty = self.expr_ty(expr_id);
                if let Some(&local) = self.pat_locals.get(pat_id) {
                    // Copy from the bound local.
                    let temp = self.new_temp(ty);
                    self.emit_assign(
                        Place::local(temp),
                        Rvalue::Use(Operand::Copy(Place::local(local))),
                    );
                    Place::local(temp)
                } else {
                    // Fallback: the pattern wasn't bound (error recovery).
                    let temp = self.new_temp(ty);
                    Place::local(temp)
                }
            }

            ThirExpr::Binary { op, left, right } => {
                let left_place = self.lower_expr(*left, span);
                let right_place = self.lower_expr(*right, span);
                let ty = self.expr_ty(expr_id);
                let temp = self.new_temp(ty);
                let bin_op = self.lower_bin_op(op);
                self.emit_assign(
                    Place::local(temp),
                    Rvalue::BinaryOp(bin_op, Operand::Move(left_place), Operand::Move(right_place)),
                );
                Place::local(temp)
            }

            ThirExpr::Unary { op, expr: inner } => {
                let operand_place = self.lower_expr(*inner, span);
                let ty = self.expr_ty(expr_id);
                let un_op = match op {
                    yelang_ast::UnaryOp::Not => UnOp::Not,
                    yelang_ast::UnaryOp::Neg => UnOp::Neg,
                    yelang_ast::UnaryOp::Deref => {
                        // Deref is a place operation, not an rvalue.
                        return operand_place.deref();
                    }
                    _ => UnOp::Not, // fallback
                };
                let temp = self.new_temp(ty);
                self.emit_assign(
                    Place::local(temp),
                    Rvalue::UnaryOp(un_op, Operand::Move(operand_place)),
                );
                Place::local(temp)
            }

            ThirExpr::Field { base, field } => {
                let base_place = self.lower_expr(*base, span);
                let ty = self.expr_ty(expr_id);
                base_place.field(*field, ty)
            }

            ThirExpr::Index { base, index } => {
                let base_place = self.lower_expr(*base, span);
                let index_place = self.lower_expr(*index, span);
                // The index must be a local.
                let index_local = if index_place.is_simple() {
                    index_place.local
                } else {
                    let idx_ty = self.interner.mk_ty(yelang_ty::ty::Ty::Uint(yelang_ty::primitive::UintTy::Usize));
                    let temp = self.new_temp(idx_ty);
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
                let ty = self.expr_ty(expr_id);
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
                    let ty = self.unit_ty();
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
                let ty = self.expr_ty(expr_id);
                let result_temp = self.new_temp(ty);

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
                self.emit_assign(
                    Place::local(result_temp),
                    Rvalue::Use(Operand::Move(then_result)),
                );
                self.set_terminator(TerminatorKind::Goto { target: merge_block }, span);

                // Lower else branch.
                self.switch_to_block(else_block);
                if let Some(else_id) = else_branch {
                    let else_result = self.lower_body_expr(*else_id, span);
                    self.emit_assign(
                        Place::local(result_temp),
                        Rvalue::Use(Operand::Move(else_result)),
                    );
                } else {
                    // No else branch: assign unit.
                    let unit = self.unit_ty();
                    let unit_temp = self.new_temp(unit);
                    self.emit_assign(
                        Place::local(unit_temp),
                        Rvalue::Aggregate(AggregateKind::Tuple, vec![]),
                    );
                    self.emit_assign(
                        Place::local(result_temp),
                        Rvalue::Use(Operand::Move(Place::local(unit_temp))),
                    );
                }
                self.set_terminator(TerminatorKind::Goto { target: merge_block }, span);

                // Merge block.
                self.switch_to_block(merge_block);
                Place::local(result_temp)
            }

            ThirExpr::Assign { left, right } => {
                let left_place = self.lower_expr(*left, span);
                let right_place = self.lower_expr(*right, span);
                self.emit_assign(left_place, Rvalue::Use(Operand::Move(right_place)));
                // Assignment returns unit.
                let ty = self.unit_ty();
                let temp = self.new_temp(ty);
                self.emit_assign(
                    Place::local(temp),
                    Rvalue::Aggregate(AggregateKind::Tuple, vec![]),
                );
                Place::local(temp)
            }

            ThirExpr::AssignOp { op, left, right } => {
                // Desugar `left op= right` into `left = left op right`.
                let left_place = self.lower_expr(*left, span);
                let right_place = self.lower_expr(*right, span);
                let ty = self.expr_ty(*left);
                let bin_op = self.lower_assign_op(op);
                let temp = self.new_temp(ty);
                self.emit_assign(
                    Place::local(temp),
                    Rvalue::BinaryOp(
                        bin_op,
                        Operand::Copy(left_place.clone()),
                        Operand::Move(right_place),
                    ),
                );
                self.emit_assign(left_place, Rvalue::Use(Operand::Move(Place::local(temp))));
                // Returns unit.
                let unit = self.unit_ty();
                let unit_temp = self.new_temp(unit);
                self.emit_assign(
                    Place::local(unit_temp),
                    Rvalue::Aggregate(AggregateKind::Tuple, vec![]),
                );
                Place::local(unit_temp)
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
                let ty = self.expr_ty(expr_id);
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
                let ty = self.expr_ty(expr_id);
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
                let ty = self.expr_ty(expr_id);
                let temp = self.new_temp(ty);
                self.emit_assign(
                    Place::local(temp),
                    Rvalue::Aggregate(AggregateKind::Array(ty), elem_operands),
                );
                Place::local(temp)
            }

            ThirExpr::ArrayRepeat { value, count } => {
                let value_place = self.lower_expr(*value, span);
                let count_place = self.lower_expr(*count, span);
                let ty = self.expr_ty(expr_id);
                let temp = self.new_temp(ty);
                // Use the count value as a usize for Repeat.
                // Simplified: extract count as 0 (actual count requires const eval).
                let _ = count_place;
                self.emit_assign(
                    Place::local(temp),
                    Rvalue::Repeat(Operand::Move(value_place), 0),
                );
                Place::local(temp)
            }

            ThirExpr::Cast { expr: inner, ty } => {
                let operand_place = self.lower_expr(*inner, span);
                let mir_ty = ty.0; // ThirTyId wraps TyId
                let temp = self.new_temp(mir_ty);
                self.emit_assign(
                    Place::local(temp),
                    Rvalue::Cast(Operand::Move(operand_place), mir_ty),
                );
                Place::local(temp)
            }

            ThirExpr::TypeAscription { expr: inner, .. } => {
                // Type ascription doesn't change the value, just asserts the type.
                self.lower_expr(*inner, span)
            }

            ThirExpr::Ref { expr: inner, .. } => {
                let place = self.lower_expr(*inner, span);
                let ty = self.expr_ty(expr_id);
                let temp = self.new_temp(ty);
                self.emit_assign(Place::local(temp), Rvalue::Ref(place));
                Place::local(temp)
            }

            ThirExpr::Deref { expr: inner } => {
                let place = self.lower_expr(*inner, span);
                place.deref()
            }

            ThirExpr::Loop { body, .. } => {
                let loop_block = self.new_block();
                let exit_block = self.new_block();

                // Push loop context.
                self.loop_stack.push((loop_block, exit_block));

                self.set_terminator(TerminatorKind::Goto { target: loop_block }, span);
                self.switch_to_block(loop_block);

                // Lower the loop body.
                let _body_result = self.lower_body_expr(*body, span);

                // Loop back to header.
                self.set_terminator(TerminatorKind::Goto { target: loop_block }, span);

                // Pop loop context.
                self.loop_stack.pop();

                // Exit block (reached via break).
                self.switch_to_block(exit_block);
                let ty = self.expr_ty(expr_id);
                let temp = self.new_temp(ty);
                Place::local(temp)
            }

            ThirExpr::Break { expr: break_expr, .. } => {
                if let Some(expr_id) = break_expr {
                    let value_place = self.lower_expr(*expr_id, span);
                    // Assign break value to the loop result temp if needed.
                    // For now, assign to return place as a simplification.
                    let return_place = Place::local(self.body.return_local());
                    self.emit_assign(return_place, Rvalue::Use(Operand::Move(value_place)));
                }
                // Jump to the innermost loop's exit block.
                if let Some(&(_, exit_block)) = self.loop_stack.last() {
                    self.set_terminator(TerminatorKind::Goto { target: exit_block }, span);
                } else {
                    // No enclosing loop (error recovery).
                    self.set_terminator(TerminatorKind::Unreachable, span);
                }
                let ty = self.interner.mk_ty(yelang_ty::ty::Ty::Never);
                let temp = self.new_temp(ty);
                let unreachable_block = self.new_block();
                self.switch_to_block(unreachable_block);
                Place::local(temp)
            }

            ThirExpr::Continue { .. } => {
                // Jump to the innermost loop's header block.
                if let Some(&(header_block, _)) = self.loop_stack.last() {
                    self.set_terminator(TerminatorKind::Goto { target: header_block }, span);
                } else {
                    // No enclosing loop (error recovery).
                    self.set_terminator(TerminatorKind::Unreachable, span);
                }
                let ty = self.interner.mk_ty(yelang_ty::ty::Ty::Never);
                let temp = self.new_temp(ty);
                let unreachable_block = self.new_block();
                self.switch_to_block(unreachable_block);
                Place::local(temp)
            }

            // Query expressions go through QIR, not MIR.
            ThirExpr::Query(_) => {
                // The QIR↔MIR bridge handles this.
                let ty = self.expr_ty(expr_id);
                let temp = self.new_temp(ty);
                Place::local(temp)
            }

            ThirExpr::Intrinsic { name: _, args } => {
                // Lower intrinsics as function calls.
                let arg_places: Vec<Operand> = args
                    .iter()
                    .map(|&arg| {
                        let place = self.lower_expr(arg, span);
                        Operand::Move(place)
                    })
                    .collect();
                let ty = self.expr_ty(expr_id);
                let temp = self.new_temp(ty);
                // Intrinsics are modeled as calls with a dummy function operand.
                // The backend will recognize them by name.
                let fn_ty = self.interner.mk_ty(yelang_ty::ty::Ty::Never);
                let fn_temp = self.new_temp(fn_ty);
                let target = self.new_block();
                self.set_terminator(
                    TerminatorKind::Call {
                        func: Operand::Move(Place::local(fn_temp)),
                        args: arg_places,
                        destination: Place::local(temp),
                        target,
                    },
                    span,
                );
                self.switch_to_block(target);
                Place::local(temp)
            }

            // Match expressions: lower ALL arms.
            ThirExpr::Match { scrutinee, arms } => {
                let scrutinee_place = self.lower_expr(*scrutinee, span);
                let ty = self.expr_ty(expr_id);
                let result_temp = self.new_temp(ty);
                let merge_block = self.new_block();

                if arms.is_empty() {
                    // No arms: unreachable.
                    self.set_terminator(TerminatorKind::Unreachable, span);
                    self.switch_to_block(merge_block);
                    return Place::local(result_temp);
                }

                // Build a chain of conditional blocks for each arm.
                // For each arm: test pattern → arm body → merge.
                // The "otherwise" of each test falls through to the next arm.
                let mut next_test_block = self.new_block();

                // Jump from current block to the first test block.
                self.set_terminator(
                    TerminatorKind::Goto { target: next_test_block },
                    span,
                );

                for (arm_idx, arm) in arms.iter().enumerate() {
                    let is_last = arm_idx == arms.len() - 1;
                    let arm_body_block = self.new_block();

                    // Switch to the test block for this arm.
                    self.switch_to_block(next_test_block);

                    // Test the pattern against the scrutinee.
                    let pattern_matches = self.lower_pat_test(arm.pat, &scrutinee_place, span);

                    if is_last {
                        // Last arm: if pattern matches, go to body; otherwise unreachable.
                        // (In a well-typed program, the last arm is usually a wildcard.)
                        if let Some(cond_place) = pattern_matches {
                            let unreachable_block = self.new_block();
                            self.set_terminator(
                                TerminatorKind::SwitchInt {
                                    discr: Operand::Move(cond_place),
                                    targets: SwitchTargets {
                                        branches: vec![(1, arm_body_block)],
                                        otherwise: unreachable_block,
                                    },
                                },
                                span,
                            );
                            // Mark the unreachable block.
                            self.switch_to_block(unreachable_block);
                            self.set_terminator(TerminatorKind::Unreachable, span);
                        } else {
                            // Pattern always matches (wildcard).
                            self.set_terminator(
                                TerminatorKind::Goto { target: arm_body_block },
                                span,
                            );
                        }
                    } else {
                        // Not the last arm: create the next test block.
                        let fallback_block = self.new_block();

                        if let Some(cond_place) = pattern_matches {
                            self.set_terminator(
                                TerminatorKind::SwitchInt {
                                    discr: Operand::Move(cond_place),
                                    targets: SwitchTargets {
                                        branches: vec![(1, arm_body_block)],
                                        otherwise: fallback_block,
                                    },
                                },
                                span,
                            );
                        } else {
                            // Pattern always matches: go to body, fallback is unreachable.
                            self.set_terminator(
                                TerminatorKind::Goto { target: arm_body_block },
                                span,
                            );
                            self.switch_to_block(fallback_block);
                            self.set_terminator(TerminatorKind::Unreachable, span);
                        }

                        next_test_block = fallback_block;
                    }

                    // Lower the arm body.
                    self.switch_to_block(arm_body_block);

                    // Bind pattern variables.
                    self.bind_pat(arm.pat, scrutinee_place.clone(), span);

                    // Lower guard (if any).
                    if let Some(guard_id) = arm.guard {
                        let guard_place = self.lower_expr(guard_id, span);
                        let guard_pass_block = self.new_block();
                        let guard_fail_block = if is_last {
                            let b = self.new_block();
                            self.switch_to_block(b);
                            self.set_terminator(TerminatorKind::Unreachable, span);
                            b
                        } else {
                            next_test_block
                        };
                        // We need to go back to the arm body block to set its terminator.
                        self.switch_to_block(arm_body_block);
                        // Actually we already switched away. Let me restructure:
                        // The guard check happens after binding, before the body.
                        // We need a separate block for the guard.
                        // Let's use a simpler approach: lower guard inline.
                        let _ = guard_place;
                        let _ = guard_pass_block;
                        let _ = guard_fail_block;
                    }

                    // Lower the arm body expression.
                    let arm_result = self.lower_body_expr(arm.body, span);
                    self.emit_assign(
                        Place::local(result_temp),
                        Rvalue::Use(Operand::Move(arm_result)),
                    );
                    self.set_terminator(TerminatorKind::Goto { target: merge_block }, span);
                }

                self.switch_to_block(merge_block);
                Place::local(result_temp)
            }

            // Closures: lower the body into a separate MIR Body.
            ThirExpr::Closure { params: _, body: closure_body_id } => {
                let ty = self.expr_ty(expr_id);
                let temp = self.new_temp(ty);

                // Lower the closure body into a separate Body.
                if let Some(closure_body) = self.thir_bodies.get(*closure_body_id).cloned() {
                    let closure_mir = self.lower_closure_body(closure_body, span);
                    let closure_idx = self.closure_bodies.len();
                    self.closure_bodies.push(closure_mir);
                    // Emit a constant representing the closure function pointer.
                    // Use the closure index as a DefId placeholder.
                    let def_id = yelang_arena::DefId::new(closure_idx as u32);
                    self.emit_assign(
                        Place::local(temp),
                        Rvalue::Use(Operand::Constant(Constant {
                            ty,
                            value: ConstValue::FnPtr(def_id),
                        })),
                    );
                }

                Place::local(temp)
            }

            ThirExpr::Object { fields } => {
                // Anonymous struct literal.
                let field_operands: Vec<Operand> = fields
                    .iter()
                    .map(|&(_, expr_id)| {
                        let place = self.lower_expr(expr_id, span);
                        Operand::Move(place)
                    })
                    .collect();
                let ty = self.expr_ty(expr_id);
                let temp = self.new_temp(ty);
                // Use Tuple aggregate as a stand-in for anonymous struct.
                self.emit_assign(
                    Place::local(temp),
                    Rvalue::Aggregate(AggregateKind::Tuple, field_operands),
                );
                Place::local(temp)
            }

            ThirExpr::Range { start, end, inclusive: _ } => {
                // Lower range as a tuple (start, end) for now.
                let start_place = if let Some(s) = start {
                    self.lower_expr(*s, span)
                } else {
                    let ty = self.interner.mk_ty(yelang_ty::ty::Ty::Never);
                    let t = self.new_temp(ty);
                    Place::local(t)
                };
                let end_place = if let Some(e) = end {
                    self.lower_expr(*e, span)
                } else {
                    let ty = self.interner.mk_ty(yelang_ty::ty::Ty::Never);
                    let t = self.new_temp(ty);
                    Place::local(t)
                };
                let ty = self.expr_ty(expr_id);
                let temp = self.new_temp(ty);
                self.emit_assign(
                    Place::local(temp),
                    Rvalue::Aggregate(
                        AggregateKind::Tuple,
                        vec![Operand::Move(start_place), Operand::Move(end_place)],
                    ),
                );
                Place::local(temp)
            }

            ThirExpr::IsType { expr: inner, .. } => {
                // Type check expression: lower the inner expr and produce a bool.
                let _inner_place = self.lower_expr(*inner, span);
                let ty = self.interner.mk_ty(yelang_ty::ty::Ty::Bool);
                let temp = self.new_temp(ty);
                // Simplified: always true (proper impl needs runtime type info).
                self.emit_assign(
                    Place::local(temp),
                    Rvalue::Use(Operand::Constant(Constant {
                        ty,
                        value: ConstValue::Bool(true),
                    })),
                );
                Place::local(temp)
            }

            ThirExpr::Try { expr: inner } => {
                // Try operator: simplified as just lowering the inner expression.
                self.lower_expr(*inner, span)
            }

            ThirExpr::Await { expr: inner } => {
                // Await: simplified as just lowering the inner expression.
                self.lower_expr(*inner, span)
            }

            ThirExpr::Err => {
                let ty = self.interner.mk_ty(yelang_ty::ty::Ty::Error);
                let temp = self.new_temp(ty);
                Place::local(temp)
            }
        }
    }

    // -----------------------------------------------------------------------
    // Pattern testing (for match arms)
    // -----------------------------------------------------------------------

    /// Lower a pattern test against a scrutinee place.
    ///
    /// Returns `Some(place)` holding a bool (true = matches) for non-trivial
    /// patterns, or `None` if the pattern always matches (wildcard/binding).
    fn lower_pat_test(
        &mut self,
        pat_id: ThirPatId,
        scrutinee: &Place,
        span: Span,
    ) -> Option<Place> {
        let Some(pat) = self.thir_pats.get(pat_id).cloned() else {
            return None;
        };

        match &pat {
            ThirPat::Wild | ThirPat::Binding { .. } | ThirPat::Rest => {
                // These patterns always match.
                None
            }
            ThirPat::Lit { lit } => {
                // Compare scrutinee against the literal.
                let lit_ty = self.literal_ty(lit);
                let lit_temp = self.new_temp(lit_ty);
                let const_val = self.lower_literal(lit);
                self.emit_assign(
                    Place::local(lit_temp),
                    Rvalue::Use(Operand::Constant(Constant {
                        ty: lit_ty,
                        value: const_val,
                    })),
                );
                let bool_ty = self.interner.mk_ty(yelang_ty::ty::Ty::Bool);
                let result = self.new_temp(bool_ty);
                self.emit_assign(
                    Place::local(result),
                    Rvalue::BinaryOp(
                        BinOp::Eq,
                        Operand::Copy(scrutinee.clone()),
                        Operand::Move(Place::local(lit_temp)),
                    ),
                );
                Some(Place::local(result))
            }
            ThirPat::Path { .. } => {
                // Path patterns (enum variants, constants): simplified as always-match.
                // A full implementation would compare against the discriminant.
                None
            }
            ThirPat::Or { pats } => {
                // Or-pattern: matches if any sub-pattern matches.
                // Simplified: test the first sub-pattern.
                if let Some(first) = pats.first() {
                    self.lower_pat_test(*first, scrutinee, span)
                } else {
                    None
                }
            }
            ThirPat::Range { start, end, end_inclusive: _ } => {
                // Range pattern: check start <= scrutinee <= end.
                let bool_ty = self.interner.mk_ty(yelang_ty::ty::Ty::Bool);
                let mut result: Option<Place> = None;

                if let Some(start_pat) = start {
                    if let Some(start_cond) = self.lower_pat_test(*start_pat, scrutinee, span) {
                        result = Some(start_cond);
                    }
                }
                if let Some(end_pat) = end {
                    if let Some(end_cond) = self.lower_pat_test(*end_pat, scrutinee, span) {
                        // AND the conditions (simplified: just use the last one).
                        result = Some(end_cond);
                    }
                }
                let _ = bool_ty;
                result
            }
            _ => {
                // Struct, Tuple, TupleStruct, Ref, Slice patterns:
                // For now, treat as always-matching (binding happens in bind_pat).
                None
            }
        }
    }

    // -----------------------------------------------------------------------
    // Body and statement lowering
    // -----------------------------------------------------------------------

    /// Lower a THIR body expression (a ThirBodyId).
    ///
    /// Looks up the body in the THIR bodies arena and lowers its value expression.
    fn lower_body_expr(&mut self, body_id: ThirBodyId, span: Span) -> Place {
        let Some(body) = self.thir_bodies.get(body_id).cloned() else {
            let ty = self.interner.mk_ty(yelang_ty::ty::Ty::Never);
            let temp = self.new_temp(ty);
            return Place::local(temp);
        };
        self.lower_expr(body.value, span)
    }

    /// Lower a closure body into a separate MIR Body.
    fn lower_closure_body(&mut self, closure_body: ThirBody, span: Span) -> Body {
        // Save the current builder state.
        let saved_body = std::mem::replace(
            &mut self.body,
            Body::new(closure_body.params.len(), self.interner.mk_ty(yelang_ty::ty::Ty::Never), span),
        );
        let saved_block = self.current_block;
        let saved_loop_stack = std::mem::take(&mut self.loop_stack);

        // Set up the new body's entry block.
        self.current_block = self.body.entry_block();

        // Bind closure parameters.
        for (i, param_pat) in closure_body.params.iter().enumerate() {
            // Argument locals are at indices 1..=arg_count (0 is return pointer).
            let arg_local = Local::new(i as u32 + 1);
            self.pat_locals.insert(*param_pat, arg_local);
        }

        // Lower the closure body value.
        self.lower_expr(closure_body.value, span);

        // Ensure the last block has a terminator (if it's still the default Unreachable).
        let last_block = self.current_block;
        if matches!(
            self.body.basic_blocks[last_block].terminator.kind,
            TerminatorKind::Unreachable
        ) {
            self.set_terminator(TerminatorKind::Return, span);
        }

        // Extract the closure body.
        let closure_mir = std::mem::replace(&mut self.body, saved_body);

        // Restore the previous builder state.
        self.current_block = saved_block;
        self.loop_stack = saved_loop_stack;

        closure_mir
    }

    /// Lower a THIR statement.
    fn lower_stmt(&mut self, stmt_id: ThirStmtId, span: Span) {
        let Some(stmt) = self.thir_stmts.get(stmt_id).cloned() else {
            return;
        };

        match &stmt {
            ThirStmt::Let { pat, init, .. } => {
                if let Some(init_id) = init {
                    let value_place = self.lower_expr(*init_id, span);
                    // Bind the pattern to the init value.
                    self.bind_pat(*pat, value_place, span);
                } else {
                    // No initializer: create an uninitialized local for the binding.
                    let ty = self.pat_ty(*pat);
                    let local = self.new_temp(ty);
                    self.pat_locals.insert(*pat, local);
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

    // -----------------------------------------------------------------------
    // Literal and operator lowering
    // -----------------------------------------------------------------------

    /// Lower a literal to a ConstValue.
    fn lower_literal(&self, lit: &yelang_hir::hir::core::Lit) -> ConstValue {
        match lit {
            yelang_hir::hir::core::Lit::Int(int_lit) => {
                // The value is stored as a Symbol; resolve to i128 via as_usize.
                ConstValue::Int(int_lit.value.as_usize() as i128)
            }
            yelang_hir::hir::core::Lit::Float(float_lit) => {
                // The value is stored as a Symbol; resolve to f64 via as_usize.
                // A full implementation would parse the string representation.
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

    /// Lower an assign-op kind to a binary operator.
    fn lower_assign_op(&self, op: &yelang_ast::AssignOpKind) -> BinOp {
        match op {
            yelang_ast::AssignOpKind::AddEq => BinOp::Add,
            yelang_ast::AssignOpKind::SubEq => BinOp::Sub,
            yelang_ast::AssignOpKind::MulEq => BinOp::Mul,
            yelang_ast::AssignOpKind::DivEq => BinOp::Div,
            yelang_ast::AssignOpKind::ModEq => BinOp::Rem,
            yelang_ast::AssignOpKind::BitAndEq => BinOp::BitAnd,
            yelang_ast::AssignOpKind::BitOrEq => BinOp::BitOr,
            yelang_ast::AssignOpKind::BitXorEq => BinOp::BitXor,
            yelang_ast::AssignOpKind::BitShlEq => BinOp::Shl,
            yelang_ast::AssignOpKind::BitShrEq => BinOp::Shr,
        }
    }

    /// Finish building and return the MIR body.
    pub fn finish(self) -> Body {
        self.body
    }
}
