/*! Expression and statement type checking.
 *
 * The main type checker that infers types for all expressions and statements
 * within a function body.
 */

use yelang_ast::BinaryOp;
use yelang_hir::hir::{Arm, Block, Expr, ExprKind, FieldExpr, Lit, Stmt, StmtKind};
use yelang_hir::hir_pat::Pat;
use yelang_hir::res::Res;
use yelang_ty::generic::GenericArg;
use yelang_ty::list::List;
use yelang_ty::primitive::{FloatTy, IntTy};
use yelang_ty::ty::{InferTy, Mutability, Ty, TyKind, TypeAndMut};
use yelang_util::HirId;

use yelang_infer::error::TypeError;

use crate::coerce::Coerce;
use crate::fn_ctxt::{BreakableKind, BreakableScope, FnCtxt};
use crate::hir_ty_lower::lower_hir_ty;
use crate::pat::check_pat;

/// Type-check a function body.
pub fn check_body<'tcx>(fcx: &mut FnCtxt<'tcx>, body: &yelang_hir::hir_body::Body) {
    fcx.push_scope();

    // Check parameters: introduce local variables for each param
    for param in &body.params {
        let param_ty = lower_hir_ty(&param.ty, fcx);
        check_pat(fcx, &param.pat, param_ty);
    }

    // Check the body expression
    let body_ty = check_expr(fcx, &body.value);

    // Coerce body type to return type
    let _ = fcx.coerce(body_ty, fcx.return_ty);

    fcx.pop_scope();
}

/// Type-check an expression and return its inferred type.
pub fn check_expr<'tcx>(fcx: &mut FnCtxt<'tcx>, expr: &Expr) -> Ty<'tcx> {
    let ty = check_expr_kind(fcx, &expr.kind, expr.hir_id);
    fcx.record_expr_ty(expr.hir_id, ty);
    ty
}

fn check_expr_kind<'tcx>(fcx: &mut FnCtxt<'tcx>, kind: &ExprKind, hir_id: HirId) -> Ty<'tcx> {
    match kind {
        ExprKind::Lit { lit } => check_literal(fcx, lit),
        ExprKind::Path { res } => check_path(fcx, res),
        ExprKind::Binary { op, left, right } => check_binary(fcx, *op, left, right),
        ExprKind::Unary { op, expr } => check_unary(fcx, *op, expr),
        ExprKind::Call { func, args } => check_call(fcx, func, args),
        ExprKind::MethodCall { receiver, method: _, args, .. } => {
            check_method_call(fcx, receiver, args)
        }
        ExprKind::Field { expr, field } => check_field(fcx, expr, field),
        ExprKind::Index { expr, index } => check_index(fcx, expr, index),
        ExprKind::Assign { left, right } => check_assign(fcx, left, right),
        ExprKind::Block { block } => check_block(fcx, block),
        ExprKind::Loop { block, label } => check_loop(fcx, block, label.as_ref()),
        ExprKind::Break { label, expr } => check_break(fcx, label.as_ref(), expr.as_deref()),
        ExprKind::Continue { label } => check_continue(fcx, label.as_ref()),
        ExprKind::Return { expr } => check_return(fcx, expr.as_deref()),
        ExprKind::Match { expr, arms } => check_match(fcx, expr, arms),
        ExprKind::If { cond, then_branch, else_branch } => {
            check_if(fcx, cond, then_branch, else_branch.as_deref())
        }
        ExprKind::Let { pat, expr } => check_let_expr(fcx, pat, expr),
        ExprKind::Closure { params, body, .. } => check_closure(fcx, params, *body),
        ExprKind::Struct { path, fields, rest } => check_struct_literal(fcx, path, fields, rest.as_deref()),
        ExprKind::Tuple { exprs } => check_tuple(fcx, exprs),
        ExprKind::Array { exprs } => check_array(fcx, exprs),
        ExprKind::Cast { expr, ty } => check_cast(fcx, expr, ty),
        ExprKind::Err => fcx.mk_error(),
    }
}

// ---------------------------------------------------------------------------
// Literal checking
// ---------------------------------------------------------------------------

fn check_literal<'tcx>(fcx: &mut FnCtxt<'tcx>, lit: &Lit) -> Ty<'tcx> {
    match lit {
        Lit::Int(_) => fcx.new_int_var(),
        Lit::Float(_) => fcx.new_float_var(),
        Lit::Bool(_) => fcx.mk_bool(),
        Lit::Char(_) => fcx.mk_char(),
        Lit::Str(_) => fcx.mk_str(),
        Lit::Regex(_) | Lit::DateTime(_) | Lit::Duration(_) | Lit::Uuid(_) |
        Lit::Bytes(_) | Lit::Geometry(_) | Lit::RecordId(_) | Lit::Unit => {
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
        Res::Local { hir_id } => {
            if let Some(ty) = fcx.lookup_local(*hir_id) {
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

fn check_binary<'tcx>(fcx: &mut FnCtxt<'tcx>, op: BinaryOp, left: &Expr, right: &Expr) -> Ty<'tcx> {
    let left_ty = check_expr(fcx, left);
    let right_ty = check_expr(fcx, right);

    match op {
        // Arithmetic: both operands must be numeric, result is same type
        BinaryOp::Add | BinaryOp::Subtract | BinaryOp::Multiply | BinaryOp::Divide | BinaryOp::Modulo | BinaryOp::Power => {
            let _ = fcx.eq(left_ty, right_ty);
            left_ty
        }
        // Bitwise: both operands must be integer, result is same type
        BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor | BinaryOp::Shl | BinaryOp::Shr => {
            let _ = fcx.eq(left_ty, right_ty);
            left_ty
        }
        // Comparison: both operands must be comparable, result is bool
        BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Lte | BinaryOp::Gt | BinaryOp::Gte | BinaryOp::Like | BinaryOp::ILike | BinaryOp::Regex | BinaryOp::In | BinaryOp::NotIn => {
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

fn check_unary<'tcx>(fcx: &mut FnCtxt<'tcx>, op: yelang_ast::UnaryOp, expr: &Expr) -> Ty<'tcx> {
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
                _ => {
                    fcx.mk_error()
                }
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

fn check_call<'tcx>(fcx: &mut FnCtxt<'tcx>, func: &Expr, args: &[Expr]) -> Ty<'tcx> {
    let func_ty = check_expr(fcx, func);

    match func_ty.kind() {
        TyKind::FnPtr(sig) => {
            let inputs = &sig.sig.inputs;
            let output = sig.sig.output;

            if inputs.len() != args.len() {
                return fcx.mk_error();
            }

            for (input, arg) in inputs.iter().zip(args.iter()) {
                let arg_ty = check_expr(fcx, arg);
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
            let arg_tys: Vec<_> = args.iter().map(|arg| check_expr(fcx, arg)).collect();
            let arg_args = fcx.interner.mk_generic_args(
                &arg_tys.iter().map(|&t| GenericArg::Type(t)).collect::<Vec<_>>(),
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

fn check_method_call<'tcx>(fcx: &mut FnCtxt<'tcx>, receiver: &Expr, args: &[Expr]) -> Ty<'tcx> {
    let _receiver_ty = check_expr(fcx, receiver);
    for arg in args {
        let _ = check_expr(fcx, arg);
    }
    // TODO: method lookup
    fcx.new_ty_var()
}

// ---------------------------------------------------------------------------
// Field access checking
// ---------------------------------------------------------------------------

fn check_field<'tcx>(fcx: &mut FnCtxt<'tcx>, expr: &Expr, field: &yelang_ast::Ident) -> Ty<'tcx> {
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

fn check_index<'tcx>(fcx: &mut FnCtxt<'tcx>, expr: &Expr, index: &Expr) -> Ty<'tcx> {
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

fn check_assign<'tcx>(fcx: &mut FnCtxt<'tcx>, left: &Expr, right: &Expr) -> Ty<'tcx> {
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
        check_stmt(fcx, stmt);
    }

    let ty = if let Some(expr) = &block.expr {
        check_expr(fcx, expr)
    } else {
        fcx.mk_unit()
    };

    fcx.pop_scope();
    ty
}

fn check_stmt<'tcx>(fcx: &mut FnCtxt<'tcx>, stmt: &Stmt) {
    match &stmt.kind {
        StmtKind::Expr { expr } => {
            let _ = check_expr(fcx, expr);
        }
        StmtKind::Let { pat, ty, init } => {
            let init_ty = if let Some(init_expr) = init {
                check_expr(fcx, init_expr)
            } else {
                fcx.new_ty_var()
            };

            let expected_ty = if let Some(hir_ty) = ty {
                let annotated = lower_hir_ty(hir_ty, fcx);
                let _ = fcx.eq(annotated, init_ty);
                annotated
            } else {
                init_ty
            };

            check_pat(fcx, pat, expected_ty);
        }
        StmtKind::Item { .. } => {
            // Nested items are checked separately
        }
    }
}

// ---------------------------------------------------------------------------
// Loop checking
// ---------------------------------------------------------------------------

fn check_loop<'tcx>(fcx: &mut FnCtxt<'tcx>, block: &Block, label: Option<&yelang_ast::Label>) -> Ty<'tcx> {
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

fn check_break<'tcx>(fcx: &mut FnCtxt<'tcx>, label: Option<&yelang_ast::Label>, expr: Option<&Expr>) -> Ty<'tcx> {
    let breakable_idx = if let Some(lbl) = label {
        fcx.breakable_scopes.iter().rposition(|s| {
            s.label.as_ref().map(|l| l.symbol.as_usize()) == Some(lbl.symbol.as_usize())
        })
    } else {
        fcx.breakable_scopes.iter().rposition(|s| s.kind == BreakableKind::Loop)
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
        fcx.breakable_scopes.iter().rev().find(|s| {
            s.label.as_ref().map(|l| l.symbol.as_usize()) == Some(lbl.symbol.as_usize())
        })
    } else {
        fcx.breakable_scopes.iter().rev().find(|s| s.kind == BreakableKind::Loop)
    };
    fcx.mk_never()
}

// ---------------------------------------------------------------------------
// Return checking
// ---------------------------------------------------------------------------

fn check_return<'tcx>(fcx: &mut FnCtxt<'tcx>, expr: Option<&Expr>) -> Ty<'tcx> {
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

fn check_match<'tcx>(fcx: &mut FnCtxt<'tcx>, expr: &Expr, arms: &[Arm]) -> Ty<'tcx> {
    let scrutinee_ty = check_expr(fcx, expr);
    let result_ty = fcx.new_ty_var();

    for arm in arms {
        check_pat(fcx, &arm.pat, scrutinee_ty);
        if let Some(guard) = &arm.guard {
            let guard_ty = check_expr(fcx, guard);
            let _ = fcx.eq(guard_ty, fcx.mk_bool());
        }
        let body_ty = check_expr(fcx, &arm.body);
        let _ = fcx.eq(result_ty, body_ty);
    }

    result_ty
}

// ---------------------------------------------------------------------------
// If checking
// ---------------------------------------------------------------------------

fn check_if<'tcx>(fcx: &mut FnCtxt<'tcx>, cond: &Expr, then_branch: &Expr, else_branch: Option<&Expr>) -> Ty<'tcx> {
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

fn check_let_expr<'tcx>(fcx: &mut FnCtxt<'tcx>, pat: &Pat, expr: &Expr) -> Ty<'tcx> {
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
    body_id: yelang_hir::ids::BodyId,
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
    rest: Option<&Expr>,
) -> Ty<'tcx> {
    let struct_ty = check_path(fcx, path);

    for field in fields {
        let _field_ty = check_expr(fcx, &field.expr);
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

fn check_tuple<'tcx>(fcx: &mut FnCtxt<'tcx>, exprs: &[Expr]) -> Ty<'tcx> {
    let tys: Vec<_> = exprs.iter().map(|e| check_expr(fcx, e)).collect();
    let args = fcx.interner.mk_generic_args(
        &tys.iter().map(|&t| GenericArg::Type(t)).collect::<Vec<_>>(),
    );
    fcx.mk_ty(TyKind::Tuple(args))
}

// ---------------------------------------------------------------------------
// Array checking
// ---------------------------------------------------------------------------

fn check_array<'tcx>(fcx: &mut FnCtxt<'tcx>, exprs: &[Expr]) -> Ty<'tcx> {
    if exprs.is_empty() {
        let elem_ty = fcx.new_ty_var();
        let len = yelang_ty::ty::Const { kind: yelang_ty::ty::ConstKind::Value(yelang_ty::ty::ConstValue::Int(0)), ty: fcx.mk_int(IntTy::I32) };
        return fcx.mk_array(elem_ty, len);
    }

    let first_ty = check_expr(fcx, &exprs[0]);
    for expr in exprs.iter().skip(1) {
        let ty = check_expr(fcx, expr);
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

fn check_cast<'tcx>(fcx: &mut FnCtxt<'tcx>, expr: &Expr, ty: &yelang_hir::hir_ty::Ty) -> Ty<'tcx> {
    let _expr_ty = check_expr(fcx, expr);
    lower_hir_ty(ty, fcx)
}
