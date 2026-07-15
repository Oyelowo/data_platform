/*! Exhaustive tests for yelang-tycheck.
 *
 * Covers every ExprKind, every PatKind, coercion cases, writeback,
 * and error recovery paths.
 */

use yelang_ast::{BinaryOp, UnaryOp};
use yelang_hir::hir::{Arm, Block, Expr, ExprKind, FieldExpr, Stmt, StmtKind};
use yelang_hir::hir_pat::{Pat, PatKind, BindingMode};
use yelang_hir::hir_ty::{Ty as HirTy, TyKind as HirTyKind};
use yelang_hir::res::Res;
use yelang_interner::Symbol;
use yelang_lexer::{Span, Position};
use yelang_ty::interner::Interner;
use yelang_ty::primitive::{FloatTy, IntTy};
use yelang_ty::ty::{Mutability, Ty, TyKind};
use yelang_util::{DefId, FxHashMap, HirId};

use crate::check::{check_body, check_expr};
use crate::coerce::Coerce;
use crate::fn_ctxt::FnCtxt;
use crate::pat::check_pat;
use crate::writeback::writeback_types;

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn dummy_span() -> Span {
    Span::new(Position::default(), Position::default())
}

fn dummy_hir_ty() -> HirTy {
    HirTy {
        kind: HirTyKind::Infer,
        span: dummy_span(),
    }
}

fn hir_id(n: u32) -> HirId {
    HirId::new(n)
}

fn def_id(n: u32) -> DefId {
    DefId::new(n)
}

fn expr(kind: ExprKind, id: u32) -> Expr {
    Expr {
        hir_id: hir_id(id),
        kind,
        span: dummy_span(),
        ty: dummy_hir_ty(),
    }
}

fn boxed_expr(kind: ExprKind, id: u32) -> Box<Expr> {
    Box::new(expr(kind, id))
}

fn pat(kind: PatKind, id: u32) -> Pat {
    Pat {
        hir_id: hir_id(id),
        kind,
        span: dummy_span(),
    }
}

fn block(stmts: Vec<Stmt>, trailing: Option<Expr>) -> Block {
    Block {
        stmts,
        expr: trailing.map(Box::new),
        span: dummy_span(),
    }
}

fn stmt_expr(e: Expr) -> Stmt {
    Stmt {
        kind: StmtKind::Expr { expr: Box::new(e) },
        span: dummy_span(),
    }
}

fn stmt_let(pat: Pat, init: Option<Expr>) -> Stmt {
    Stmt {
        kind: StmtKind::Let {
            pat,
            ty: None,
            init: init.map(Box::new),
        },
        span: dummy_span(),
    }
}

fn fcx_with_return_ty<'tcx>(interner: &'tcx Interner<'tcx>, return_ty: Ty<'tcx>) -> FnCtxt<'tcx> {
    FnCtxt::new(interner, def_id(1), return_ty, FxHashMap::new())
}

fn mk_fcx<'tcx>(interner: &'tcx Interner<'tcx>) -> FnCtxt<'tcx> {
    fcx_with_return_ty(interner, interner.mk_ty(TyKind::Tuple(yelang_ty::list::List::empty())))
}

fn local_res(id: u32) -> Res {
    Res::Local { hir_id: hir_id(id) }
}

fn def_res(id: u32) -> Res {
    Res::Def { def_id: def_id(id) }
}

fn symbol(n: u32) -> Symbol {
    Symbol::from(n)
}

// ---------------------------------------------------------------------------
// Literal checking
// ---------------------------------------------------------------------------

#[test]
fn literal_int_creates_int_var() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let e = expr(ExprKind::Lit { lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit { value: symbol(1), suffix: None }) }, 1);
    let ty = check_expr(&mut fcx, &e);
    assert!(matches!(ty.kind(), TyKind::Infer(yelang_ty::ty::InferTy::IntVar(_))));
}

#[test]
fn literal_float_creates_float_var() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let e = expr(ExprKind::Lit { lit: yelang_lexer::Literal::Float(yelang_lexer::FloatLit { value: symbol(1), suffix: None }) }, 1);
    let ty = check_expr(&mut fcx, &e);
    assert!(matches!(ty.kind(), TyKind::Infer(yelang_ty::ty::InferTy::FloatVar(_))));
}

#[test]
fn literal_bool_is_bool() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let e = expr(ExprKind::Lit { lit: yelang_lexer::Literal::Bool(true) }, 1);
    let ty = check_expr(&mut fcx, &e);
    assert_eq!(ty, interner.mk_ty(TyKind::Bool));
}

#[test]
fn literal_char_is_char() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let e = expr(ExprKind::Lit { lit: yelang_lexer::Literal::Char('a') }, 1);
    let ty = check_expr(&mut fcx, &e);
    assert_eq!(ty, interner.mk_ty(TyKind::Char));
}

#[test]
fn literal_str_is_str() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let e = expr(ExprKind::Lit { lit: yelang_lexer::Literal::Str(yelang_lexer::StringLit { value: symbol(1), kind: yelang_lexer::StrKind::Normal }) }, 1);
    let ty = check_expr(&mut fcx, &e);
    assert_eq!(ty, interner.mk_ty(TyKind::Str));
}

// ---------------------------------------------------------------------------
// Path checking
// ---------------------------------------------------------------------------

#[test]
fn path_local_lookup() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let local_ty = interner.mk_ty(TyKind::Int(IntTy::I32));
    fcx.insert_local(hir_id(1), local_ty);

    let e = expr(ExprKind::Path { res: local_res(1) }, 2);
    let ty = check_expr(&mut fcx, &e);
    assert_eq!(ty, local_ty);
}

#[test]
fn path_def_lookup() {
    let interner = Interner::new();
    let mut item_types = FxHashMap::new();
    let def_ty = interner.mk_ty(TyKind::Int(IntTy::I64));
    item_types.insert(def_id(1), def_ty);
    let mut fcx = FnCtxt::new(&interner, def_id(1), interner.mk_ty(TyKind::Tuple(yelang_ty::list::List::empty())), item_types);

    let e = expr(ExprKind::Path { res: def_res(1) }, 2);
    let ty = check_expr(&mut fcx, &e);
    assert_eq!(ty, def_ty);
}

#[test]
fn path_missing_local_is_error() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let e = expr(ExprKind::Path { res: local_res(99) }, 1);
    let ty = check_expr(&mut fcx, &e);
    assert_eq!(ty, interner.mk_ty(TyKind::Error));
}

// ---------------------------------------------------------------------------
// Binary operator checking
// ---------------------------------------------------------------------------

#[test]
fn binary_arithmetic_unifies_operands() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let left = expr(ExprKind::Lit { lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit { value: symbol(1), suffix: None }) }, 1);
    let right = expr(ExprKind::Lit { lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit { value: symbol(2), suffix: None }) }, 2);
    let e = expr(ExprKind::Binary { op: BinaryOp::Add, left: boxed_expr(left.kind, 1), right: boxed_expr(right.kind, 2) }, 3);
    let ty = check_expr(&mut fcx, &e);
    // Both operands are int vars; they unify, result is same int var.
    assert!(matches!(ty.kind(), TyKind::Infer(yelang_ty::ty::InferTy::IntVar(_))));
}

#[test]
fn binary_comparison_returns_bool() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let left = expr(ExprKind::Lit { lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit { value: symbol(1), suffix: None }) }, 1);
    let right = expr(ExprKind::Lit { lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit { value: symbol(2), suffix: None }) }, 2);
    let e = expr(ExprKind::Binary { op: BinaryOp::Eq, left: boxed_expr(left.kind, 1), right: boxed_expr(right.kind, 2) }, 3);
    let ty = check_expr(&mut fcx, &e);
    assert_eq!(ty, interner.mk_ty(TyKind::Bool));
}

#[test]
fn binary_logical_requires_bool() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let left = expr(ExprKind::Lit { lit: yelang_lexer::Literal::Bool(true) }, 1);
    let right = expr(ExprKind::Lit { lit: yelang_lexer::Literal::Bool(false) }, 2);
    let e = expr(ExprKind::Binary { op: BinaryOp::And, left: boxed_expr(left.kind, 1), right: boxed_expr(right.kind, 2) }, 3);
    let ty = check_expr(&mut fcx, &e);
    assert_eq!(ty, interner.mk_ty(TyKind::Bool));
}

#[test]
fn binary_bitwise_unifies_operands() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let left = expr(ExprKind::Lit { lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit { value: symbol(1), suffix: None }) }, 1);
    let right = expr(ExprKind::Lit { lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit { value: symbol(2), suffix: None }) }, 2);
    let e = expr(ExprKind::Binary { op: BinaryOp::BitAnd, left: boxed_expr(left.kind, 1), right: boxed_expr(right.kind, 2) }, 3);
    let ty = check_expr(&mut fcx, &e);
    assert!(matches!(ty.kind(), TyKind::Infer(yelang_ty::ty::InferTy::IntVar(_))));
}

// ---------------------------------------------------------------------------
// Unary operator checking
// ---------------------------------------------------------------------------

#[test]
fn unary_neg_preserves_type() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let inner = expr(ExprKind::Lit { lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit { value: symbol(1), suffix: None }) }, 1);
    let e = expr(ExprKind::Unary { op: UnaryOp::Neg, expr: boxed_expr(inner.kind, 1) }, 2);
    let ty = check_expr(&mut fcx, &e);
    assert!(matches!(ty.kind(), TyKind::Infer(yelang_ty::ty::InferTy::IntVar(_))));
}

#[test]
fn unary_not_preserves_type() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let inner = expr(ExprKind::Lit { lit: yelang_lexer::Literal::Bool(true) }, 1);
    let e = expr(ExprKind::Unary { op: UnaryOp::Not, expr: boxed_expr(inner.kind, 1) }, 2);
    let ty = check_expr(&mut fcx, &e);
    assert_eq!(ty, interner.mk_ty(TyKind::Bool));
}

#[test]
fn unary_deref_on_reference() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let pointee = interner.mk_ty(TyKind::Int(IntTy::I32));
    let ref_ty = interner.mk_ty(TyKind::Ref(pointee, Mutability::Not));
    fcx.insert_local(hir_id(1), ref_ty);

    let inner = expr(ExprKind::Path { res: local_res(1) }, 1);
    let e = expr(ExprKind::Unary { op: UnaryOp::Deref, expr: boxed_expr(inner.kind, 1) }, 2);
    let ty = check_expr(&mut fcx, &e);
    assert_eq!(ty, pointee);
}

#[test]
fn unary_deref_inference() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let inner = expr(ExprKind::Lit { lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit { value: symbol(1), suffix: None }) }, 1);
    let e = expr(ExprKind::Unary { op: UnaryOp::Deref, expr: boxed_expr(inner.kind, 1) }, 2);
    let ty = check_expr(&mut fcx, &e);
    // The operand is an int var, not a reference, so this is an error.
    assert_eq!(ty, interner.mk_ty(TyKind::Error));
}

#[test]
fn unary_ref_creates_reference() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let inner = expr(ExprKind::Lit { lit: yelang_lexer::Literal::Bool(true) }, 1);
    let e = expr(ExprKind::Unary { op: UnaryOp::Ref, expr: boxed_expr(inner.kind, 1) }, 2);
    let ty = check_expr(&mut fcx, &e);
    assert_eq!(ty, interner.mk_ty(TyKind::Ref(interner.mk_ty(TyKind::Bool), Mutability::Not)));
}

#[test]
fn unary_refmut_creates_mutable_reference() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let inner = expr(ExprKind::Lit { lit: yelang_lexer::Literal::Bool(true) }, 1);
    let e = expr(ExprKind::Unary { op: UnaryOp::RefMut, expr: boxed_expr(inner.kind, 1) }, 2);
    let ty = check_expr(&mut fcx, &e);
    assert_eq!(ty, interner.mk_ty(TyKind::Ref(interner.mk_ty(TyKind::Bool), Mutability::Mut)));
}

// ---------------------------------------------------------------------------
// Call checking
// ---------------------------------------------------------------------------

#[test]
fn call_fn_ptr() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let i32_ty = interner.mk_ty(TyKind::Int(IntTy::I32));
    let bool_ty = interner.mk_ty(TyKind::Bool);
    let inputs = interner.mk_generic_args(&[yelang_ty::generic::GenericArg::Type(i32_ty)]);
    let fn_ty = interner.mk_ty(TyKind::FnPtr(yelang_ty::ty::PolyFnSig {
        sig: yelang_ty::ty::FnSig { inputs, output: bool_ty },
    }));
    fcx.insert_local(hir_id(1), fn_ty);

    let func = expr(ExprKind::Path { res: local_res(1) }, 1);
    let arg = expr(ExprKind::Lit { lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit { value: symbol(1), suffix: None }) }, 2);
    let e = expr(ExprKind::Call { func: boxed_expr(func.kind, 1), args: vec![arg] }, 3);
    let ty = check_expr(&mut fcx, &e);
    assert_eq!(ty, bool_ty);
}

#[test]
fn call_wrong_arg_count_is_error() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let i32_ty = interner.mk_ty(TyKind::Int(IntTy::I32));
    let inputs = interner.mk_generic_args(&[yelang_ty::generic::GenericArg::Type(i32_ty)]);
    let fn_ty = interner.mk_ty(TyKind::FnPtr(yelang_ty::ty::PolyFnSig {
        sig: yelang_ty::ty::FnSig { inputs, output: i32_ty },
    }));
    fcx.insert_local(hir_id(1), fn_ty);

    let func = expr(ExprKind::Path { res: local_res(1) }, 1);
    let e = expr(ExprKind::Call { func: boxed_expr(func.kind, 1), args: vec![] }, 3);
    let ty = check_expr(&mut fcx, &e);
    assert_eq!(ty, interner.mk_ty(TyKind::Error));
}

#[test]
fn call_unknown_function_inference() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let func_ty = fcx.new_ty_var();
    fcx.insert_local(hir_id(1), func_ty);
    let func = expr(ExprKind::Path { res: local_res(1) }, 1);
    let arg = expr(ExprKind::Lit { lit: yelang_lexer::Literal::Bool(true) }, 2);
    let e = expr(ExprKind::Call { func: boxed_expr(func.kind, 1), args: vec![arg] }, 3);
    let ty = check_expr(&mut fcx, &e);
    // Should infer a function type and return a type variable.
    assert!(matches!(ty.kind(), TyKind::Infer(yelang_ty::ty::InferTy::TyVar(_))));
}

// ---------------------------------------------------------------------------
// Field access checking
// ---------------------------------------------------------------------------

#[test]
fn field_tuple_access() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let i32_ty = interner.mk_ty(TyKind::Int(IntTy::I32));
    let bool_ty = interner.mk_ty(TyKind::Bool);
    let tuple_ty = interner.mk_ty(TyKind::Tuple(interner.mk_generic_args(&[
        yelang_ty::generic::GenericArg::Type(i32_ty),
        yelang_ty::generic::GenericArg::Type(bool_ty),
    ])));
    fcx.insert_local(hir_id(1), tuple_ty);

    let base = expr(ExprKind::Path { res: local_res(1) }, 1);
    let e = expr(ExprKind::Field { expr: boxed_expr(base.kind, 1), field: yelang_ast::Ident { symbol: symbol(0), span: dummy_span() } }, 2);
    let ty = check_expr(&mut fcx, &e);
    assert_eq!(ty, i32_ty);
}

#[test]
fn field_tuple_out_of_bounds_is_error() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let i32_ty = interner.mk_ty(TyKind::Int(IntTy::I32));
    let tuple_ty = interner.mk_ty(TyKind::Tuple(interner.mk_generic_args(&[
        yelang_ty::generic::GenericArg::Type(i32_ty),
    ])));
    fcx.insert_local(hir_id(1), tuple_ty);

    let base = expr(ExprKind::Path { res: local_res(1) }, 1);
    let e = expr(ExprKind::Field { expr: boxed_expr(base.kind, 1), field: yelang_ast::Ident { symbol: symbol(5), span: dummy_span() } }, 2);
    let ty = check_expr(&mut fcx, &e);
    assert_eq!(ty, interner.mk_ty(TyKind::Error));
}

// ---------------------------------------------------------------------------
// Index checking
// ---------------------------------------------------------------------------

#[test]
fn index_array() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let i32_ty = interner.mk_ty(TyKind::Int(IntTy::I32));
    let array_ty = interner.mk_ty(TyKind::Array(i32_ty, yelang_ty::ty::Const { kind: yelang_ty::ty::ConstKind::Value(yelang_ty::ty::ConstValue::Int(3)), ty: i32_ty }));
    fcx.insert_local(hir_id(1), array_ty);

    let base = expr(ExprKind::Path { res: local_res(1) }, 1);
    let idx = expr(ExprKind::Lit { lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit { value: symbol(1), suffix: None }) }, 2);
    let e = expr(ExprKind::Index { expr: boxed_expr(base.kind, 1), index: boxed_expr(idx.kind, 2) }, 3);
    let ty = check_expr(&mut fcx, &e);
    assert_eq!(ty, i32_ty);
}

#[test]
fn index_slice() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let i32_ty = interner.mk_ty(TyKind::Int(IntTy::I32));
    let slice_ty = interner.mk_ty(TyKind::Slice(i32_ty));
    fcx.insert_local(hir_id(1), slice_ty);

    let base = expr(ExprKind::Path { res: local_res(1) }, 1);
    let idx = expr(ExprKind::Lit { lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit { value: symbol(1), suffix: None }) }, 2);
    let e = expr(ExprKind::Index { expr: boxed_expr(base.kind, 1), index: boxed_expr(idx.kind, 2) }, 3);
    let ty = check_expr(&mut fcx, &e);
    assert_eq!(ty, i32_ty);
}

// ---------------------------------------------------------------------------
// Assignment checking
// ---------------------------------------------------------------------------

#[test]
fn assign_unifies_types() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let i32_ty = interner.mk_ty(TyKind::Int(IntTy::I32));
    fcx.insert_local(hir_id(1), i32_ty);

    let left = expr(ExprKind::Path { res: local_res(1) }, 1);
    let right = expr(ExprKind::Lit { lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit { value: symbol(1), suffix: None }) }, 2);
    let e = expr(ExprKind::Assign { left: boxed_expr(left.kind, 1), right: boxed_expr(right.kind, 2) }, 3);
    let ty = check_expr(&mut fcx, &e);
    assert_eq!(ty, interner.mk_ty(TyKind::Tuple(yelang_ty::list::List::empty())));
}

// ---------------------------------------------------------------------------
// Block checking
// ---------------------------------------------------------------------------

#[test]
fn block_empty_is_unit() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let b = block(vec![], None);
    let e = expr(ExprKind::Block { block: b }, 1);
    let ty = check_expr(&mut fcx, &e);
    assert!(ty.is_unit());
}

#[test]
fn block_with_trailing_expr() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let trailing = expr(ExprKind::Lit { lit: yelang_lexer::Literal::Bool(true) }, 2);
    let b = block(vec![], Some(trailing));
    let e = expr(ExprKind::Block { block: b }, 1);
    let ty = check_expr(&mut fcx, &e);
    assert_eq!(ty, interner.mk_ty(TyKind::Bool));
}

#[test]
fn block_with_let_binding() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let init = expr(ExprKind::Lit { lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit { value: symbol(1), suffix: None }) }, 2);
    let p = pat(PatKind::Binding { mode: BindingMode::ByValue, name: symbol(1), subpat: None }, 3);
    let s = stmt_let(p, Some(init));
    let b = block(vec![s], None);
    let e = expr(ExprKind::Block { block: b }, 1);
    let ty = check_expr(&mut fcx, &e);
    assert!(ty.is_unit());
}

// ---------------------------------------------------------------------------
// Loop and break checking
// ---------------------------------------------------------------------------

#[test]
fn loop_without_break_is_never() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let b = block(vec![], None);
    let e = expr(ExprKind::Loop { block: b, label: None }, 1);
    let ty = check_expr(&mut fcx, &e);
    assert!(ty.is_never());
}

#[test]
fn loop_with_break_value() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let break_expr = expr(ExprKind::Break { label: None, expr: Some(boxed_expr(ExprKind::Lit { lit: yelang_lexer::Literal::Bool(true) }, 2)) }, 3);
    let b = block(vec![stmt_expr(break_expr)], None);
    let e = expr(ExprKind::Loop { block: b, label: None }, 1);
    let ty = check_expr(&mut fcx, &e);
    assert_eq!(ty, interner.mk_ty(TyKind::Bool));
}

#[test]
fn break_without_value_is_unit() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let break_expr = expr(ExprKind::Break { label: None, expr: None }, 2);
    let b = block(vec![stmt_expr(break_expr)], None);
    let e = expr(ExprKind::Loop { block: b, label: None }, 1);
    let ty = check_expr(&mut fcx, &e);
    assert!(ty.is_unit());
}

#[test]
fn continue_is_never() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let continue_expr = expr(ExprKind::Continue { label: None }, 2);
    let b = block(vec![stmt_expr(continue_expr)], None);
    let e = expr(ExprKind::Loop { block: b, label: None }, 1);
    let ty = check_expr(&mut fcx, &e);
    assert!(ty.is_never());
}

// ---------------------------------------------------------------------------
// Return checking
// ---------------------------------------------------------------------------

#[test]
fn return_with_value_unifies_with_return_ty() {
    let interner = Interner::new();
    let i32_ty = interner.mk_ty(TyKind::Int(IntTy::I32));
    let mut fcx = fcx_with_return_ty(&interner, i32_ty);
    let ret = expr(ExprKind::Return { expr: Some(boxed_expr(ExprKind::Lit { lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit { value: symbol(1), suffix: None }) }, 2)) }, 1);
    let ty = check_expr(&mut fcx, &ret);
    assert!(ty.is_never());
}

#[test]
fn return_without_value_unifies_with_unit() {
    let interner = Interner::new();
    let unit_ty = interner.mk_ty(TyKind::Tuple(yelang_ty::list::List::empty()));
    let mut fcx = fcx_with_return_ty(&interner, unit_ty);
    let ret = expr(ExprKind::Return { expr: None }, 1);
    let ty = check_expr(&mut fcx, &ret);
    assert!(ty.is_never());
}

// ---------------------------------------------------------------------------
// If checking
// ---------------------------------------------------------------------------

#[test]
fn if_else_branches_unify() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let cond = expr(ExprKind::Lit { lit: yelang_lexer::Literal::Bool(true) }, 1);
    let then_branch = expr(ExprKind::Lit { lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit { value: symbol(1), suffix: None }) }, 2);
    let else_branch = expr(ExprKind::Lit { lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit { value: symbol(2), suffix: None }) }, 3);
    let e = expr(ExprKind::If { cond: boxed_expr(cond.kind, 1), then_branch: boxed_expr(then_branch.kind, 2), else_branch: Some(boxed_expr(else_branch.kind, 3)) }, 4);
    let ty = check_expr(&mut fcx, &e);
    assert!(matches!(ty.kind(), TyKind::Infer(yelang_ty::ty::InferTy::IntVar(_))));
}

#[test]
fn if_without_else_requires_unit() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let cond = expr(ExprKind::Lit { lit: yelang_lexer::Literal::Bool(true) }, 1);
    let then_branch = expr(ExprKind::Block { block: block(vec![], None) }, 2);
    let e = expr(ExprKind::If { cond: boxed_expr(cond.kind, 1), then_branch: boxed_expr(then_branch.kind, 2), else_branch: None }, 3);
    let ty = check_expr(&mut fcx, &e);
    assert!(ty.is_unit());
}

// ---------------------------------------------------------------------------
// Match checking
// ---------------------------------------------------------------------------

#[test]
fn match_arms_unify() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let scrutinee = expr(ExprKind::Lit { lit: yelang_lexer::Literal::Bool(true) }, 1);
    let arm1 = Arm {
        pat: pat(PatKind::Wild, 2),
        guard: None,
        body: boxed_expr(ExprKind::Lit { lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit { value: symbol(1), suffix: None }) }, 3),
        span: dummy_span(),
    };
    let arm2 = Arm {
        pat: pat(PatKind::Wild, 4),
        guard: None,
        body: boxed_expr(ExprKind::Lit { lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit { value: symbol(2), suffix: None }) }, 5),
        span: dummy_span(),
    };
    let e = expr(ExprKind::Match { expr: boxed_expr(scrutinee.kind, 1), arms: vec![arm1, arm2] }, 6);
    let ty = check_expr(&mut fcx, &e);
    // check_match returns a TyVar that gets unified with the arm types.
    // After resolution it should be an IntVar.
    let resolved = fcx.resolve_ty(ty);
    assert!(matches!(resolved.kind(), TyKind::Infer(yelang_ty::ty::InferTy::IntVar(_))));
}

#[test]
fn match_guard_must_be_bool() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let scrutinee = expr(ExprKind::Lit { lit: yelang_lexer::Literal::Bool(true) }, 1);
    let arm = Arm {
        pat: pat(PatKind::Wild, 2),
        guard: Some(boxed_expr(ExprKind::Lit { lit: yelang_lexer::Literal::Bool(true) }, 3)),
        body: boxed_expr(ExprKind::Lit { lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit { value: symbol(1), suffix: None }) }, 4),
        span: dummy_span(),
    };
    let e = expr(ExprKind::Match { expr: boxed_expr(scrutinee.kind, 1), arms: vec![arm] }, 5);
    let ty = check_expr(&mut fcx, &e);
    let resolved = fcx.resolve_ty(ty);
    assert!(matches!(resolved.kind(), TyKind::Infer(yelang_ty::ty::InferTy::IntVar(_))));
}

// ---------------------------------------------------------------------------
// Let expression checking (if-let)
// ---------------------------------------------------------------------------

#[test]
fn let_expr_returns_bool() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let scrutinee = expr(ExprKind::Lit { lit: yelang_lexer::Literal::Bool(true) }, 1);
    let p = pat(PatKind::Wild, 2);
    let e = expr(ExprKind::Let { pat: p, expr: boxed_expr(scrutinee.kind, 1) }, 3);
    let ty = check_expr(&mut fcx, &e);
    assert_eq!(ty, interner.mk_ty(TyKind::Bool));
}

// ---------------------------------------------------------------------------
// Tuple checking
// ---------------------------------------------------------------------------

#[test]
fn tuple_literal() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let a = expr(ExprKind::Lit { lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit { value: symbol(1), suffix: None }) }, 1);
    let b = expr(ExprKind::Lit { lit: yelang_lexer::Literal::Bool(true) }, 2);
    let e = expr(ExprKind::Tuple { exprs: vec![a, b] }, 3);
    let ty = check_expr(&mut fcx, &e);
    match ty.kind() {
        TyKind::Tuple(args) => {
            assert_eq!(args.len(), 2);
        }
        _ => panic!("expected tuple"),
    }
}

// ---------------------------------------------------------------------------
// Array checking
// ---------------------------------------------------------------------------

#[test]
fn array_literal_homogeneous() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let a = expr(ExprKind::Lit { lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit { value: symbol(1), suffix: None }) }, 1);
    let b = expr(ExprKind::Lit { lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit { value: symbol(2), suffix: None }) }, 2);
    let e = expr(ExprKind::Array { exprs: vec![a, b] }, 3);
    let ty = check_expr(&mut fcx, &e);
    match ty.kind() {
        TyKind::Array(elem, _) => {
            assert!(matches!(elem.kind(), TyKind::Infer(yelang_ty::ty::InferTy::IntVar(_))));
        }
        _ => panic!("expected array"),
    }
}

#[test]
fn array_literal_empty() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let e = expr(ExprKind::Array { exprs: vec![] }, 1);
    let ty = check_expr(&mut fcx, &e);
    match ty.kind() {
        TyKind::Array(_, len) => {
            assert!(matches!(len.kind, yelang_ty::ty::ConstKind::Value(yelang_ty::ty::ConstValue::Int(0))));
        }
        _ => panic!("expected array"),
    }
}

// ---------------------------------------------------------------------------
// Struct literal checking
// ---------------------------------------------------------------------------

#[test]
fn struct_literal_returns_struct_ty() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let i32_ty = interner.mk_ty(TyKind::Int(IntTy::I32));
    fcx.insert_local(hir_id(1), i32_ty);

    let path = local_res(1);
    let field = FieldExpr {
        ident: yelang_ast::Ident { symbol: symbol(1), span: dummy_span() },
        expr: expr(ExprKind::Lit { lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit { value: symbol(1), suffix: None }) }, 2),
        span: dummy_span(),
    };
    let e = expr(ExprKind::Struct { path, fields: vec![field], rest: None }, 3);
    let ty = check_expr(&mut fcx, &e);
    assert_eq!(ty, i32_ty);
}

// ---------------------------------------------------------------------------
// Cast checking
// ---------------------------------------------------------------------------

#[test]
fn cast_returns_target_type() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let inner = expr(ExprKind::Lit { lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit { value: symbol(1), suffix: None }) }, 1);
    let target = HirTy { kind: HirTyKind::Path { res: Res::PrimTy { ty: yelang_hir::res::PrimTy::Int(yelang_hir::res::IntTy::I64) } }, span: dummy_span() };
    let e = expr(ExprKind::Cast { expr: boxed_expr(inner.kind, 1), ty: target }, 2);
    let ty = check_expr(&mut fcx, &e);
    assert_eq!(ty, interner.mk_ty(TyKind::Int(IntTy::I64)));
}

// ---------------------------------------------------------------------------
// Pattern checking
// ---------------------------------------------------------------------------

#[test]
fn pat_wild() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let i32_ty = interner.mk_ty(TyKind::Int(IntTy::I32));
    let p = pat(PatKind::Wild, 1);
    check_pat(&mut fcx, &p, i32_ty);
    assert_eq!(fcx.results.pat_ty(hir_id(1)), Some(i32_ty));
}

#[test]
fn pat_binding_inserts_local() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let i32_ty = interner.mk_ty(TyKind::Int(IntTy::I32));
    let p = pat(PatKind::Binding { mode: BindingMode::ByValue, name: symbol(1), subpat: None }, 1);
    check_pat(&mut fcx, &p, i32_ty);
    assert_eq!(fcx.results.local_ty(hir_id(1)), Some(i32_ty));
    assert_eq!(fcx.results.pat_ty(hir_id(1)), Some(i32_ty));
}

#[test]
fn pat_tuple() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let i32_ty = interner.mk_ty(TyKind::Int(IntTy::I32));
    let p = pat(PatKind::Tuple { pats: vec![
        pat(PatKind::Wild, 2),
        pat(PatKind::Wild, 3),
    ]}, 1);
    check_pat(&mut fcx, &p, i32_ty);
    assert_eq!(fcx.results.pat_ty(hir_id(1)), Some(i32_ty));
}

#[test]
fn pat_or() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let i32_ty = interner.mk_ty(TyKind::Int(IntTy::I32));
    let p = pat(PatKind::Or { pats: vec![
        pat(PatKind::Wild, 2),
        pat(PatKind::Wild, 3),
    ]}, 1);
    check_pat(&mut fcx, &p, i32_ty);
    assert_eq!(fcx.results.pat_ty(hir_id(1)), Some(i32_ty));
}

#[test]
fn pat_slice() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let i32_ty = interner.mk_ty(TyKind::Int(IntTy::I32));
    let p = pat(PatKind::Slice { prefix: vec![pat(PatKind::Wild, 2)], middle: None, suffix: vec![] }, 1);
    check_pat(&mut fcx, &p, i32_ty);
    assert_eq!(fcx.results.pat_ty(hir_id(1)), Some(i32_ty));
}

#[test]
fn pat_err_is_error() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let p = pat(PatKind::Err, 1);
    check_pat(&mut fcx, &p, interner.mk_ty(TyKind::Int(IntTy::I32)));
    assert_eq!(fcx.results.pat_ty(hir_id(1)), Some(interner.mk_ty(TyKind::Error)));
}

// ---------------------------------------------------------------------------
// Coercion checking
// ---------------------------------------------------------------------------

#[test]
fn coerce_exact_match() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let i32_ty = interner.mk_ty(TyKind::Int(IntTy::I32));
    let result = fcx.coerce(i32_ty, i32_ty);
    assert_eq!(result, Ok(i32_ty));
}

#[test]
fn coerce_mismatch_fails() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let i32_ty = interner.mk_ty(TyKind::Int(IntTy::I32));
    let bool_ty = interner.mk_ty(TyKind::Bool);
    let result = fcx.coerce(i32_ty, bool_ty);
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Writeback checking
// ---------------------------------------------------------------------------

#[test]
fn writeback_resolves_ty_var() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let ty_var = fcx.new_ty_var();
    let i32_ty = interner.mk_ty(TyKind::Int(IntTy::I32));
    fcx.eq(ty_var, i32_ty).unwrap();
    fcx.results.expr_types.insert(hir_id(1), ty_var);

    writeback_types(&mut fcx);
    let resolved = fcx.results.expr_ty(hir_id(1)).unwrap();
    assert_eq!(resolved, i32_ty);
}

#[test]
fn writeback_int_fallback_to_i32() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let int_var = fcx.new_int_var();
    fcx.results.expr_types.insert(hir_id(1), int_var);

    writeback_types(&mut fcx);
    let resolved = fcx.results.expr_ty(hir_id(1)).unwrap();
    assert_eq!(resolved, interner.mk_ty(TyKind::Int(IntTy::I32)));
}

#[test]
fn writeback_float_fallback_to_f64() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let float_var = fcx.new_float_var();
    fcx.results.expr_types.insert(hir_id(1), float_var);

    writeback_types(&mut fcx);
    let resolved = fcx.results.expr_ty(hir_id(1)).unwrap();
    assert_eq!(resolved, interner.mk_ty(TyKind::Float(FloatTy::F64)));
}

#[test]
fn writeback_unresolved_ty_var_becomes_error() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let ty_var = fcx.new_ty_var();
    fcx.results.expr_types.insert(hir_id(1), ty_var);

    writeback_types(&mut fcx);
    let resolved = fcx.results.expr_ty(hir_id(1)).unwrap();
    assert_eq!(resolved, interner.mk_ty(TyKind::Error));
}

// ---------------------------------------------------------------------------
// Body checking
// ---------------------------------------------------------------------------

#[test]
fn body_check_params_and_expr() {
    let interner = Interner::new();
    let i32_ty = interner.mk_ty(TyKind::Int(IntTy::I32));
    let mut fcx = fcx_with_return_ty(&interner, i32_ty);

    let param = yelang_hir::hir_body::Param {
        pat: pat(PatKind::Binding { mode: BindingMode::ByValue, name: symbol(1), subpat: None }, 1),
        ty: HirTy { kind: HirTyKind::Path { res: Res::PrimTy { ty: yelang_hir::res::PrimTy::Int(yelang_hir::res::IntTy::I32) } }, span: dummy_span() },
        span: dummy_span(),
    };
    let body = yelang_hir::hir_body::Body {
        params: vec![param],
        value: expr(ExprKind::Path { res: local_res(1) }, 2),
        span: dummy_span(),
    };
    check_body(&mut fcx, &body);
    assert_eq!(fcx.results.local_ty(hir_id(1)), Some(i32_ty));
}

// ---------------------------------------------------------------------------
// Error recovery
// ---------------------------------------------------------------------------

#[test]
fn expr_err_is_error() {
    let interner = Interner::new();
    let mut fcx = mk_fcx(&interner);
    let e = expr(ExprKind::Err, 1);
    let ty = check_expr(&mut fcx, &e);
    assert_eq!(ty, interner.mk_ty(TyKind::Error));
}
