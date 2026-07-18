/*! Expression and statement type checking.
 *
 * The main type checker that infers types for all expressions and statements
 * within a function body.
 */

use yelang_ast::{AssignOpKind, BinaryOp};
use yelang_hir::hir::{Arm, Block, Expr, FieldExpr, Stmt};
use yelang_hir::ids::{BodyId, ExprId, PatId, StmtId, TyId};
use yelang_hir::res::Res;
use yelang_ty::generic::GenericArg;
use yelang_ty::primitive::IntTy;
use yelang_ty::ty::{InferTy, Mutability, Ty, TyKind, TypeAndMut};

use crate::coerce::Coerce;
use crate::fn_ctxt::{BreakableKind, BreakableScope, FnCtxt};
use crate::hir_ty_lower::lower_hir_ty;
use crate::pat::check_pat;

/// Type-check a function body.
pub fn check_body<'tcx>(fcx: &mut FnCtxt<'tcx>, body_id: BodyId) {
    fcx.push_scope();

    let body = fcx
        .crate_hir
        .bodies
        .get(body_id)
        .expect("BodyId should be valid")
        .clone();

    // Check parameters: introduce local variables for each param
    for param in &body.params {
        let param_ty = lower_hir_ty_id(fcx, param.ty);
        check_pat(fcx, param.pat, param_ty);
    }

    // Check the body expression
    let body_ty = check_expr(fcx, body.value);

    // Coerce body type to return type
    let _ = fcx.coerce(body_ty, fcx.return_ty);

    fcx.pop_scope();
}

/// Type-check an expression and return its inferred type.
pub fn check_expr<'tcx>(fcx: &mut FnCtxt<'tcx>, expr_id: ExprId) -> Ty<'tcx> {
    let expr = fcx
        .crate_hir
        .exprs
        .get(expr_id)
        .expect("ExprId should be valid")
        .clone();
    let ty = check_expr_value(fcx, &expr, expr_id);
    fcx.record_expr_ty(expr_id, ty);
    ty
}

fn check_expr_value<'tcx>(fcx: &mut FnCtxt<'tcx>, expr: &Expr, _expr_id: ExprId) -> Ty<'tcx> {
    match expr {
        Expr::Lit { lit } => check_literal(fcx, lit),
        Expr::Path { res } => check_path(fcx, res),
        Expr::Binary { op, left, right } => check_binary(fcx, *op, *left, *right),
        Expr::Unary { op, expr } => check_unary(fcx, *op, *expr),
        Expr::Call { func, args } => check_call(fcx, *func, args),
        Expr::MethodCall {
            receiver,
            method: _,
            args,
            ..
        } => check_method_call(fcx, *receiver, args),
        Expr::Field { expr, field } => check_field(fcx, *expr, field),
        Expr::Index { expr, index } => check_index(fcx, *expr, *index),
        Expr::Assign { left, right } => check_assign(fcx, *left, *right),
        Expr::Block { block } => check_block(fcx, block),
        Expr::Loop { block, label } => check_loop(fcx, block, label.as_ref()),
        Expr::Break { label, expr } => check_break(fcx, label.as_ref(), *expr),
        Expr::Continue { label } => check_continue(fcx, label.as_ref()),
        Expr::Return { expr } => check_return(fcx, *expr),
        Expr::Match { expr, arms } => check_match(fcx, *expr, arms),
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => check_if(fcx, *cond, *then_branch, *else_branch),
        Expr::Let { pat, expr } => check_let_expr(fcx, *pat, *expr),
        Expr::Closure { params, body, .. } => check_closure(fcx, params, *body),
        Expr::Struct { path, fields, rest } => {
            check_struct_literal(fcx, path, fields, *rest)
        }
        Expr::Tuple { exprs } => check_tuple(fcx, exprs),
        Expr::Array { exprs } => check_array(fcx, exprs),
        Expr::Cast { expr, ty } => check_cast(fcx, *expr, *ty),
        Expr::AssignOp { left, right, op } => check_assign_op(fcx, op.clone(), *left, *right),
        Expr::DestructureAssign { pat, value } => {
            let value_ty = check_expr(fcx, *value);
            crate::pat::check_pat(fcx, *pat, value_ty);
            fcx.mk_unit()
        }
        Expr::Range { start, end, .. } => check_range(fcx, *start, *end),
        Expr::Object { fields } => check_object_literal(fcx, fields),
        Expr::IsType { expr: inner, ty } => {
            check_expr(fcx, *inner);
            lower_hir_ty_id(fcx, *ty)
        }
        Expr::TypeAscription { expr: inner, ty } => {
            let ascribed = lower_hir_ty_id(fcx, *ty);
            let expr_ty = check_expr(fcx, *inner);
            let _ = fcx.eq(expr_ty, ascribed);
            ascribed
        }
        Expr::Try { expr: inner } => check_try(fcx, *inner),
        Expr::Await { expr: inner } => {
            check_expr(fcx, *inner);
            fcx.new_ty_var()
        }
        Expr::Async { body } => {
            check_body(fcx, *body);
            fcx.new_ty_var()
        }
        Expr::Gen { body, .. } => {
            check_body(fcx, *body);
            fcx.new_ty_var()
        }
        Expr::DocumentAccess { base, projection } => {
            check_expr(fcx, *base);
            for proj in projection {
                match proj {
                    yelang_hir::hir_expr::DocumentProjection::Field { value, .. } => {
                        if let Some(e) = value {
                            check_expr(fcx, *e);
                        }
                    }
                    yelang_hir::hir_expr::DocumentProjection::Spread(e) => {
                        check_expr(fcx, *e);
                    }
                }
            }
            fcx.new_ty_var()
        }
        Expr::Comprehension {
            element,
            variables,
            condition,
            ..
        } => {
            for (_pat, source) in variables {
                check_expr(fcx, *source);
            }
            check_expr(fcx, *element);
            if let Some(cond) = condition {
                check_expr(fcx, *cond);
            }
            fcx.new_ty_var()
        }
        Expr::Err => fcx.mk_error(),
    }
}

// ---------------------------------------------------------------------------
// Literal checking
// ---------------------------------------------------------------------------

fn check_literal<'tcx>(fcx: &mut FnCtxt<'tcx>, lit: &yelang_hir::hir::Lit) -> Ty<'tcx> {
    match lit {
        yelang_hir::hir::Lit::Int(_) => fcx.new_int_var(),
        yelang_hir::hir::Lit::Float(_) => fcx.new_float_var(),
        yelang_hir::hir::Lit::Bool(_) => fcx.mk_bool(),
        yelang_hir::hir::Lit::Char(_) => fcx.mk_char(),
        yelang_hir::hir::Lit::Str(_) => fcx.mk_str(),
        _ => {
            // TODO: define types for these literals
            fcx.new_ty_var()
        }
    }
}

// ---------------------------------------------------------------------------
// Path checking
// ---------------------------------------------------------------------------

fn check_path<'tcx>(fcx: &mut FnCtxt<'tcx>, res: &Res) -> Ty<'tcx> {
    match res {
        Res::Local { pat_id } => {
            if let Some(ty) = fcx.lookup_local(*pat_id) {
                ty
            } else {
                fcx.mk_error()
            }
        }
        Res::Def { def_id } => {
            if let Some(ty) = fcx.item_ty(*def_id) {
                ty
            } else {
                fcx.mk_error()
            }
        }
        Res::PrimTy { .. } => {
            // PrimTy shouldn't appear in expression position
            fcx.mk_error()
        }
        Res::SelfTy { .. } => {
            if let Some(ty) = fcx.self_ty {
                ty
            } else {
                fcx.mk_error()
            }
        }
        Res::SelfVal { .. } => {
            // self parameter type
            fcx.self_ty.unwrap_or_else(|| fcx.mk_error())
        }
        Res::Err => fcx.mk_error(),
    }
}

// ---------------------------------------------------------------------------
// Binary operator checking
// ---------------------------------------------------------------------------

fn check_binary<'tcx>(
    fcx: &mut FnCtxt<'tcx>,
    op: BinaryOp,
    left: ExprId,
    right: ExprId,
) -> Ty<'tcx> {
    let left_ty = check_expr(fcx, left);
    let right_ty = check_expr(fcx, right);

    match op {
        // Arithmetic: both operands must be numeric, result is same type
        BinaryOp::Add
        | BinaryOp::Subtract
        | BinaryOp::Multiply
        | BinaryOp::Divide
        | BinaryOp::Modulo
        | BinaryOp::Power => {
            let _ = fcx.eq(left_ty, right_ty);
            left_ty
        }
        // Bitwise: both operands must be integer, result is same type
        BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor | BinaryOp::Shl | BinaryOp::Shr => {
            let _ = fcx.eq(left_ty, right_ty);
            left_ty
        }
        // Comparison: both operands must be comparable, result is bool
        BinaryOp::Eq
        | BinaryOp::Ne
        | BinaryOp::Lt
        | BinaryOp::Lte
        | BinaryOp::Gt
        | BinaryOp::Gte
        | BinaryOp::Like
        | BinaryOp::ILike
        | BinaryOp::Regex
        | BinaryOp::In
        | BinaryOp::NotIn => {
            let _ = fcx.eq(left_ty, right_ty);
            fcx.mk_bool()
        }
        // Logical: both operands must be bool, result is bool
        BinaryOp::And | BinaryOp::Or => {
            let _ = fcx.eq(left_ty, fcx.mk_bool());
            let _ = fcx.eq(right_ty, fcx.mk_bool());
            fcx.mk_bool()
        }
    }
}

// ---------------------------------------------------------------------------
// Unary operator checking
// ---------------------------------------------------------------------------

fn check_unary<'tcx>(
    fcx: &mut FnCtxt<'tcx>,
    op: yelang_ast::UnaryOp,
    expr: ExprId,
) -> Ty<'tcx> {
    let expr_ty = check_expr(fcx, expr);

    match op {
        yelang_ast::UnaryOp::Neg => {
            // Operand must be numeric; result is same type
            expr_ty
        }
        yelang_ast::UnaryOp::Not => {
            // Operand must be bool or integer; result is same type
            expr_ty
        }
        yelang_ast::UnaryOp::Deref => {
            // Operand must be pointer or reference; result is pointee
            match expr_ty.kind() {
                TyKind::Ref(ty, _) | TyKind::RawPtr(TypeAndMut { ty, .. }) => *ty,
                TyKind::Infer(InferTy::TyVar(_)) => {
                    let inner = fcx.new_ty_var();
                    let ptr = fcx.mk_ref(inner, Mutability::Not);
                    let _ = fcx.eq(expr_ty, ptr);
                    inner
                }
                _ => fcx.mk_error(),
            }
        }
        yelang_ast::UnaryOp::Ref => {
            // Result is &T (immutable reference)
            fcx.mk_ref(expr_ty, Mutability::Not)
        }
        yelang_ast::UnaryOp::RefMut => {
            // Result is &mut T
            fcx.mk_ref(expr_ty, Mutability::Mut)
        }
    }
}

// ---------------------------------------------------------------------------
// Call checking
// ---------------------------------------------------------------------------

fn check_call<'tcx>(fcx: &mut FnCtxt<'tcx>, func: ExprId, args: &[ExprId]) -> Ty<'tcx> {
    let func_ty = check_expr(fcx, func);

    match func_ty.kind() {
        TyKind::FnPtr(sig) => {
            let inputs = &sig.sig.inputs;
            let output = sig.sig.output;

            if inputs.len() != args.len() {
                return fcx.mk_error();
            }

            for (input, arg) in inputs.iter().zip(args.iter()) {
                let arg_ty = check_expr(fcx, *arg);
                let expected = match input {
                    GenericArg::Type(t) => *t,
                    _ => fcx.mk_error(),
                };
                let _ = fcx.eq(expected, arg_ty);
            }

            output
        }
        TyKind::FnDef(fd) => {
            // Similar to FnPtr but may have generic args
            // For now, treat as error
            let _ = (fd, args);
            fcx.mk_error()
        }
        TyKind::Infer(InferTy::TyVar(_)) => {
            // Function type not yet known: create expected arg types and return type
            let arg_tys: Vec<_> = args.iter().map(|arg| check_expr(fcx, *arg)).collect();
            let arg_args = fcx.interner.mk_generic_args(
                &arg_tys
                    .iter()
                    .map(|&t| GenericArg::Type(t))
                    .collect::<Vec<_>>(),
            );
            let ret_ty = fcx.new_ty_var();
            let expected = fcx.mk_fn_ptr(arg_args, ret_ty);
            let _ = fcx.eq(func_ty, expected);
            ret_ty
        }
        _ => fcx.mk_error(),
    }
}

// ---------------------------------------------------------------------------
// Method call checking
// ---------------------------------------------------------------------------

fn check_method_call<'tcx>(
    fcx: &mut FnCtxt<'tcx>,
    receiver: ExprId,
    args: &[ExprId],
) -> Ty<'tcx> {
    let _receiver_ty = check_expr(fcx, receiver);
    for arg in args {
        let _ = check_expr(fcx, *arg);
    }
    // TODO: method lookup
    fcx.new_ty_var()
}

// ---------------------------------------------------------------------------
// Field access checking
// ---------------------------------------------------------------------------

fn check_field<'tcx>(
    fcx: &mut FnCtxt<'tcx>,
    expr: ExprId,
    field: &yelang_ast::Ident,
) -> Ty<'tcx> {
    let expr_ty = check_expr(fcx, expr);

    match expr_ty.kind() {
        TyKind::Tuple(args) => {
            // Tuple field access: field name should be a digit index
            let index = field.symbol.as_usize();
            if let Some(arg) = args.get(index) {
                match arg {
                    GenericArg::Type(t) => *t,
                    _ => fcx.mk_error(),
                }
            } else {
                fcx.mk_error()
            }
        }
        TyKind::Adt(_, _) | TyKind::AnonStruct(_) | TyKind::Infer(InferTy::TyVar(_)) => {
            // TODO: struct field lookup (needs field map from DefId)
            fcx.new_ty_var()
        }
        _ => fcx.mk_error(),
    }
}

// ---------------------------------------------------------------------------
// Index checking
// ---------------------------------------------------------------------------

fn check_index<'tcx>(fcx: &mut FnCtxt<'tcx>, expr: ExprId, index: ExprId) -> Ty<'tcx> {
    let expr_ty = check_expr(fcx, expr);
    let index_ty = check_expr(fcx, index);

    // Index must be integer
    let _ = fcx.eq(index_ty, fcx.mk_int(IntTy::I32));

    match expr_ty.kind() {
        TyKind::Array(ty, _) | TyKind::Slice(ty) => *ty,
        TyKind::Infer(InferTy::TyVar(_)) => {
            let elem_ty = fcx.new_ty_var();
            let expected = fcx.mk_slice(elem_ty);
            let _ = fcx.eq(expr_ty, expected);
            elem_ty
        }
        _ => fcx.mk_error(),
    }
}

// ---------------------------------------------------------------------------
// Assignment checking
// ---------------------------------------------------------------------------

fn check_assign<'tcx>(fcx: &mut FnCtxt<'tcx>, left: ExprId, right: ExprId) -> Ty<'tcx> {
    let left_ty = check_expr(fcx, left);
    let right_ty = check_expr(fcx, right);
    let _ = fcx.eq(left_ty, right_ty);
    fcx.mk_unit()
}

// ---------------------------------------------------------------------------
// Block checking
// ---------------------------------------------------------------------------

fn check_block<'tcx>(fcx: &mut FnCtxt<'tcx>, block: &Block) -> Ty<'tcx> {
    fcx.push_scope();

    for stmt in &block.stmts {
        check_stmt(fcx, *stmt);
    }

    let ty = if let Some(expr) = &block.expr {
        check_expr(fcx, *expr)
    } else {
        fcx.mk_unit()
    };

    fcx.pop_scope();
    ty
}

fn check_stmt<'tcx>(fcx: &mut FnCtxt<'tcx>, stmt_id: StmtId) {
    let stmt = fcx
        .crate_hir
        .stmts
        .get(stmt_id)
        .expect("StmtId should be valid")
        .clone();
    match &stmt {
        Stmt::Expr { expr } => {
            let _ = check_expr(fcx, *expr);
        }
        Stmt::Let { pat, ty, init } => {
            let init_ty = if let Some(init_expr) = init {
                check_expr(fcx, *init_expr)
            } else {
                fcx.new_ty_var()
            };

            let expected_ty = if let Some(hir_ty) = ty {
                let annotated = lower_hir_ty_id(fcx, *hir_ty);
                let _ = fcx.eq(annotated, init_ty);
                annotated
            } else {
                init_ty
            };

            check_pat(fcx, *pat, expected_ty);
        }
        Stmt::Item { .. } => {
            // Nested items are checked separately
        }
    }
}

// ---------------------------------------------------------------------------
// Loop checking
// ---------------------------------------------------------------------------

fn check_loop<'tcx>(
    fcx: &mut FnCtxt<'tcx>,
    block: &Block,
    label: Option<&yelang_ast::Label>,
) -> Ty<'tcx> {
    fcx.push_breakable(BreakableScope {
        label: label.cloned(),
        kind: BreakableKind::Loop,
        expr_ty: fcx.mk_never(),
        span: block.span,
    });

    let _ = check_block(fcx, block);

    let scope = fcx.pop_breakable().unwrap();

    // If no breaks with values, loop type is never (diverges)
    // If breaks with values, type is the value type
    if scope.expr_ty.is_never() {
        fcx.mk_never()
    } else {
        scope.expr_ty
    }
}

// ---------------------------------------------------------------------------
// Break checking
// ---------------------------------------------------------------------------

fn check_break<'tcx>(
    fcx: &mut FnCtxt<'tcx>,
    label: Option<&yelang_ast::Label>,
    expr: Option<ExprId>,
) -> Ty<'tcx> {
    let breakable_idx = if let Some(lbl) = label {
        fcx.breakable_scopes.iter().rposition(|s| {
            s.label.as_ref().map(|l| l.symbol.as_usize()) == Some(lbl.symbol.as_usize())
        })
    } else {
        fcx.breakable_scopes
            .iter()
            .rposition(|s| s.kind == BreakableKind::Loop)
    };

    if let Some(idx) = breakable_idx {
        let expr_ty = if let Some(e) = expr {
            check_expr(fcx, e)
        } else {
            fcx.mk_unit()
        };

        // We need to mutate the scope, so we can't hold a reference.
        let scope = &mut fcx.breakable_scopes[idx];
        if scope.expr_ty.is_never() {
            // First break: set the scope type
            scope.expr_ty = expr_ty;
        } else {
            let scope_expr_ty = scope.expr_ty;
            let _ = fcx.eq(scope_expr_ty, expr_ty);
        }
    }

    fcx.mk_never()
}

// ---------------------------------------------------------------------------
// Continue checking
// ---------------------------------------------------------------------------

fn check_continue<'tcx>(fcx: &mut FnCtxt<'tcx>, label: Option<&yelang_ast::Label>) -> Ty<'tcx> {
    let _ = if let Some(lbl) = label {
        fcx.breakable_scopes
            .iter()
            .rev()
            .find(|s| s.label.as_ref().map(|l| l.symbol.as_usize()) == Some(lbl.symbol.as_usize()))
    } else {
        fcx.breakable_scopes
            .iter()
            .rev()
            .find(|s| s.kind == BreakableKind::Loop)
    };
    fcx.mk_never()
}

// ---------------------------------------------------------------------------
// Return checking
// ---------------------------------------------------------------------------

fn check_return<'tcx>(fcx: &mut FnCtxt<'tcx>, expr: Option<ExprId>) -> Ty<'tcx> {
    let expr_ty = if let Some(e) = expr {
        check_expr(fcx, e)
    } else {
        fcx.mk_unit()
    };

    let _ = fcx.eq(fcx.return_ty, expr_ty);
    fcx.mk_never()
}

// ---------------------------------------------------------------------------
// Match checking
// ---------------------------------------------------------------------------

fn check_match<'tcx>(fcx: &mut FnCtxt<'tcx>, expr: ExprId, arms: &[Arm]) -> Ty<'tcx> {
    let scrutinee_ty = check_expr(fcx, expr);
    let result_ty = fcx.new_ty_var();

    for arm in arms {
        check_pat(fcx, arm.pat, scrutinee_ty);
        if let Some(guard) = &arm.guard {
            let guard_ty = check_expr(fcx, *guard);
            let _ = fcx.eq(guard_ty, fcx.mk_bool());
        }
        let body_ty = check_expr(fcx, arm.body);
        let _ = fcx.eq(result_ty, body_ty);
    }

    result_ty
}

// ---------------------------------------------------------------------------
// If checking
// ---------------------------------------------------------------------------

fn check_if<'tcx>(
    fcx: &mut FnCtxt<'tcx>,
    cond: ExprId,
    then_branch: ExprId,
    else_branch: Option<ExprId>,
) -> Ty<'tcx> {
    let cond_ty = check_expr(fcx, cond);
    let _ = fcx.eq(cond_ty, fcx.mk_bool());

    let then_ty = check_expr(fcx, then_branch);

    if let Some(else_expr) = else_branch {
        let else_ty = check_expr(fcx, else_expr);
        let _ = fcx.eq(then_ty, else_ty);
        then_ty
    } else {
        // No else branch: then branch must evaluate to unit
        let _ = fcx.eq(then_ty, fcx.mk_unit());
        fcx.mk_unit()
    }
}

// ---------------------------------------------------------------------------
// Let expression checking (for if let)
// ---------------------------------------------------------------------------

fn check_let_expr<'tcx>(fcx: &mut FnCtxt<'tcx>, pat: PatId, expr: ExprId) -> Ty<'tcx> {
    let expr_ty = check_expr(fcx, expr);
    check_pat(fcx, pat, expr_ty);
    fcx.mk_bool()
}

// ---------------------------------------------------------------------------
// Closure checking
// ---------------------------------------------------------------------------

fn check_closure<'tcx>(
    fcx: &mut FnCtxt<'tcx>,
    params: &[yelang_hir::hir_body::Param],
    body_id: BodyId,
) -> Ty<'tcx> {
    let _ = (params, body_id);
    // TODO: look up body from crate, check with new FnCtxt
    fcx.new_ty_var()
}

// ---------------------------------------------------------------------------
// Struct literal checking
// ---------------------------------------------------------------------------

fn check_struct_literal<'tcx>(
    fcx: &mut FnCtxt<'tcx>,
    path: &Res,
    fields: &[FieldExpr],
    rest: Option<ExprId>,
) -> Ty<'tcx> {
    let struct_ty = check_path(fcx, path);

    for field in fields {
        let _field_ty = check_expr(fcx, field.expr);
        // TODO: check field type against struct definition
    }

    if let Some(rest_expr) = rest {
        let _ = check_expr(fcx, rest_expr);
    }

    struct_ty
}

// ---------------------------------------------------------------------------
// Tuple checking
// ---------------------------------------------------------------------------

fn check_tuple<'tcx>(fcx: &mut FnCtxt<'tcx>, exprs: &[ExprId]) -> Ty<'tcx> {
    let tys: Vec<_> = exprs.iter().map(|e| check_expr(fcx, *e)).collect();
    let args = fcx
        .interner
        .mk_generic_args(&tys.iter().map(|&t| GenericArg::Type(t)).collect::<Vec<_>>());
    fcx.mk_ty(TyKind::Tuple(args))
}

// ---------------------------------------------------------------------------
// Array checking
// ---------------------------------------------------------------------------

fn check_array<'tcx>(fcx: &mut FnCtxt<'tcx>, exprs: &[ExprId]) -> Ty<'tcx> {
    if exprs.is_empty() {
        let elem_ty = fcx.new_ty_var();
        let len = yelang_ty::ty::Const {
            kind: yelang_ty::ty::ConstKind::Value(yelang_ty::ty::ConstValue::Int(0)),
            ty: fcx.mk_int(IntTy::I32),
        };
        return fcx.mk_array(elem_ty, len);
    }

    let first_ty = check_expr(fcx, exprs[0]);
    for expr in exprs.iter().skip(1) {
        let ty = check_expr(fcx, *expr);
        let _ = fcx.eq(first_ty, ty);
    }

    let len = yelang_ty::ty::Const {
        kind: yelang_ty::ty::ConstKind::Value(yelang_ty::ty::ConstValue::Int(exprs.len() as i128)),
        ty: fcx.mk_int(IntTy::I32),
    };
    fcx.mk_array(first_ty, len)
}

// ---------------------------------------------------------------------------
// Cast checking
// ---------------------------------------------------------------------------

fn check_cast<'tcx>(fcx: &mut FnCtxt<'tcx>, expr: ExprId, ty: TyId) -> Ty<'tcx> {
    let _expr_ty = check_expr(fcx, expr);
    lower_hir_ty_id(fcx, ty)
}

// ---------------------------------------------------------------------------
// HIR type lowering helper
// ---------------------------------------------------------------------------

fn lower_hir_ty_id<'tcx>(fcx: &mut FnCtxt<'tcx>, ty_id: TyId) -> Ty<'tcx> {
    let hir_ty = fcx
        .crate_hir
        .tys
        .get(ty_id)
        .expect("TyId should be valid")
        .clone();
    lower_hir_ty(&hir_ty, fcx)
}

// ---------------------------------------------------------------------------
// Assign-op, range, object, try
// ---------------------------------------------------------------------------

fn check_assign_op<'tcx>(
    fcx: &mut FnCtxt<'tcx>,
    op: AssignOpKind,
    left: ExprId,
    right: ExprId,
) -> Ty<'tcx> {
    let bin_op = assign_op_to_bin_op(op);
    let left_ty = check_expr(fcx, left);
    let right_ty = check_expr(fcx, right);

    // Assignment operators only map to arithmetic and bitwise binary ops.
    // The result of the underlying operation must be assignable back to `left`.
    match bin_op {
        BinaryOp::Add
        | BinaryOp::Subtract
        | BinaryOp::Multiply
        | BinaryOp::Divide
        | BinaryOp::Modulo
        | BinaryOp::BitAnd
        | BinaryOp::BitOr
        | BinaryOp::BitXor
        | BinaryOp::Shl
        | BinaryOp::Shr => {
            let _ = fcx.eq(left_ty, right_ty);
        }
        _ => unreachable!("assignment operators cannot map to {bin_op:?}"),
    }

    fcx.mk_unit()
}

fn assign_op_to_bin_op(op: AssignOpKind) -> BinaryOp {
    use yelang_ast::BinaryOp;
    match op {
        AssignOpKind::AddEq => BinaryOp::Add,
        AssignOpKind::SubEq => BinaryOp::Subtract,
        AssignOpKind::MulEq => BinaryOp::Multiply,
        AssignOpKind::DivEq => BinaryOp::Divide,
        AssignOpKind::ModEq => BinaryOp::Modulo,
        AssignOpKind::BitAndEq => BinaryOp::BitAnd,
        AssignOpKind::BitOrEq => BinaryOp::BitOr,
        AssignOpKind::BitXorEq => BinaryOp::BitXor,
        AssignOpKind::BitShlEq => BinaryOp::Shl,
        AssignOpKind::BitShrEq => BinaryOp::Shr,
    }
}

fn check_range<'tcx>(
    fcx: &mut FnCtxt<'tcx>,
    start: Option<ExprId>,
    end: Option<ExprId>,
) -> Ty<'tcx> {
    if let Some(e) = start {
        let _ = check_expr(fcx, e);
    }
    if let Some(e) = end {
        let _ = check_expr(fcx, e);
    }
    // Range type is language-defined; return a fresh variable until the
    // standard library Range type is wired up.
    fcx.new_ty_var()
}

fn check_object_literal<'tcx>(fcx: &mut FnCtxt<'tcx>, fields: &[FieldExpr]) -> Ty<'tcx> {
    for field in fields {
        let _ = check_expr(fcx, field.expr);
    }
    fcx.new_ty_var()
}

fn check_try<'tcx>(fcx: &mut FnCtxt<'tcx>, expr: ExprId) -> Ty<'tcx> {
    let inner_ty = check_expr(fcx, expr);
    // `expr?` unwraps the inner Ok/Some value. Until Result is modeled,
    // return the inner type and let inference sort it out.
    inner_ty
}
