/*! Exhaustive tests for yelang-tycheck.
 *
 * Covers every ExprKind, every PatKind, coercion cases, writeback,
 * and error recovery paths.
 */

#![allow(unused_mut)]

use yelang_arena::{DefId, SecondaryMap};
use yelang_ast::{BinaryOp, UnaryOp};
use yelang_hir::Crate as HirCrate;
use yelang_hir::hir::{Arm, Block, Expr, FieldExpr, Stmt};
use yelang_hir::hir_body::{Body, Param};
use yelang_hir::hir_pat::{BindingMode, Pat};
use yelang_hir::hir_ty::Ty as HirTy;
use yelang_hir::ids::{BodyId, ExprId, PatId, StmtId, TyId};
use yelang_hir::res::Res;
use yelang_interner::Symbol;
use yelang_lexer::{Position, Span};
use yelang_ty::interner::Interner;
use yelang_ty::primitive::{FloatTy, IntTy};
use yelang_ty::ty::{Mutability, Ty, TyKind};

use crate::check::{check_body, check_expr};
use crate::coerce::Coerce;
use crate::fn_ctxt::FnCtxt;
use crate::pat::check_pat;
use crate::writeback::writeback_types;
// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn hir_crate() -> HirCrate {
    HirCrate::new(DefId::new(1))
}

fn dummy_span() -> Span {
    Span::new(Position::default(), Position::default())
}

fn expr(hir: &mut HirCrate, kind: Expr) -> ExprId {
    hir.alloc_expr(kind, dummy_span())
}

fn pat(hir: &mut HirCrate, kind: Pat) -> PatId {
    hir.alloc_pat(kind, dummy_span())
}

fn block(_hir: &mut HirCrate, stmts: Vec<StmtId>, expr: Option<ExprId>) -> Block {
    Block {
        stmts,
        expr,
        span: dummy_span(),
    }
}

fn stmt_expr(hir: &mut HirCrate, e: ExprId) -> StmtId {
    hir.alloc_stmt(Stmt::Expr { expr: e }, dummy_span())
}

fn stmt_let(hir: &mut HirCrate, pat: PatId, init: Option<ExprId>) -> StmtId {
    hir.alloc_stmt(Stmt::Let { pat, ty: None, init }, dummy_span())
}

fn body(hir: &mut HirCrate, params: Vec<Param>, value: ExprId) -> BodyId {
    hir.alloc_body(Body { params, value, span: dummy_span() }, dummy_span())
}

fn hir_ty(hir: &mut HirCrate, ty: HirTy) -> TyId {
    hir.alloc_ty(ty, dummy_span())
}

fn def_id(n: u32) -> DefId {
    DefId::new(n)
}

fn def_res(id: u32) -> Res {
    Res::Def { def_id: def_id(id) }
}

fn symbol(n: u32) -> Symbol {
    Symbol::from(n)
}

fn local_res(pat_id: PatId) -> Res {
    Res::Local { pat_id }
}

fn fcx_with_return_ty<'tcx>(
    interner: &'tcx Interner<'tcx>,
    hir: &'tcx HirCrate,
    return_ty: Ty<'tcx>,
) -> FnCtxt<'tcx> {
    FnCtxt::new(interner, hir, def_id(1), return_ty, SecondaryMap::new())
}

fn mk_fcx<'tcx>(interner: &'tcx Interner<'tcx>, hir: &'tcx HirCrate) -> FnCtxt<'tcx> {
    let unit = interner.mk_ty(TyKind::Tuple(yelang_ty::list::List::empty()));
    fcx_with_return_ty(interner, hir, unit)
}

// ---------------------------------------------------------------------------
// Literal checking
// ---------------------------------------------------------------------------

#[test]
fn literal_int_creates_int_var() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _expr1 = expr(&mut hir, Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(1),
                suffix: None,
            }),
        });
    let mut fcx = mk_fcx(&interner, &hir);
    let e = _expr1;
    let ty = check_expr(&mut fcx, e);
    assert!(matches!(
        ty.kind(),
        TyKind::Infer(yelang_ty::ty::InferTy::IntVar(_))
    ));
}

#[test]
fn literal_float_creates_float_var() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _expr1 = expr(&mut hir, Expr::Lit {
            lit: yelang_lexer::Literal::Float(yelang_lexer::FloatLit {
                value: symbol(1),
                suffix: None,
            }),
        });
    let mut fcx = mk_fcx(&interner, &hir);
    let e = _expr1;
    let ty = check_expr(&mut fcx, e);
    assert!(matches!(
        ty.kind(),
        TyKind::Infer(yelang_ty::ty::InferTy::FloatVar(_))
    ));
}

#[test]
fn literal_bool_is_bool() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _expr1 = expr(&mut hir, Expr::Lit {
            lit: yelang_lexer::Literal::Bool(true),
        });
    let mut fcx = mk_fcx(&interner, &hir);
    let e = _expr1;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, interner.mk_ty(TyKind::Bool));
}

#[test]
fn literal_char_is_char() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _expr1 = expr(&mut hir, Expr::Lit {
            lit: yelang_lexer::Literal::Char('a'),
        });
    let mut fcx = mk_fcx(&interner, &hir);
    let e = _expr1;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, interner.mk_ty(TyKind::Char));
}

#[test]
fn literal_str_is_str() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _expr1 = expr(&mut hir, Expr::Lit {
            lit: yelang_lexer::Literal::Str(yelang_lexer::StringLit {
                value: symbol(1),
                kind: yelang_lexer::StrKind::Normal,
            }),
        });
    let mut fcx = mk_fcx(&interner, &hir);
    let e = _expr1;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, interner.mk_ty(TyKind::Str));
}

// ---------------------------------------------------------------------------
// Path checking
// ---------------------------------------------------------------------------

#[test]
fn path_local_lookup() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _pat1 = pat(&mut hir, Pat::Err);
    let _expr2 = expr(&mut hir, Expr::Path { res: local_res(_pat1) });
    let mut fcx = mk_fcx(&interner, &hir);
    let local_ty = interner.mk_ty(TyKind::Int(IntTy::I32));
    fcx.insert_local(_pat1, local_ty);

    let e = _expr2;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, local_ty);
}

#[test]
fn path_def_lookup() {
    let interner = Interner::new();
    let mut item_types = SecondaryMap::new();
    let def_ty = interner.mk_ty(TyKind::Int(IntTy::I64));
    item_types.insert(def_id(1), def_ty);
    let mut hir = hir_crate();
    let _expr2 = expr(&mut hir, Expr::Path { res: def_res(1) });

    let mut fcx = FnCtxt::new(
        &interner,
        &hir,
        def_id(1),
        interner.mk_ty(TyKind::Tuple(yelang_ty::list::List::empty())),
        item_types,
    );

    let e = _expr2;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, def_ty);
}

#[test]
fn path_missing_local_is_error() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _pat99 = pat(&mut hir, Pat::Err);
    let _expr1 = expr(&mut hir, Expr::Path { res: local_res(_pat99) });
    let mut fcx = mk_fcx(&interner, &hir);
    let e = _expr1;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, interner.mk_ty(TyKind::Error));
}

// ---------------------------------------------------------------------------
// Binary operator checking
// ---------------------------------------------------------------------------

#[test]
fn binary_arithmetic_unifies_operands() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _expr1 = expr(&mut hir, Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(1),
                suffix: None,
            }),
        });
    let _expr2 = expr(&mut hir, Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(2),
                suffix: None,
            }),
        });
    let left = _expr1;
    let right = _expr2;
    let _expr3 = expr(&mut hir, Expr::Binary {
            op: BinaryOp::Add,
            left: left,
            right: right,
        });
    let mut fcx = mk_fcx(&interner, &hir);


    let e = _expr3;
    let ty = check_expr(&mut fcx, e);
    // Both operands are int vars; they unify, result is same int var.
    assert!(matches!(
        ty.kind(),
        TyKind::Infer(yelang_ty::ty::InferTy::IntVar(_))
    ));
}

#[test]
fn binary_comparison_returns_bool() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _expr1 = expr(&mut hir, Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(1),
                suffix: None,
            }),
        });
    let _expr2 = expr(&mut hir, Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(2),
                suffix: None,
            }),
        });
    let left = _expr1;
    let right = _expr2;
    let _expr3 = expr(&mut hir, Expr::Binary {
            op: BinaryOp::Eq,
            left: left,
            right: right,
        });
    let mut fcx = mk_fcx(&interner, &hir);


    let e = _expr3;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, interner.mk_ty(TyKind::Bool));
}

#[test]
fn binary_logical_requires_bool() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _expr1 = expr(&mut hir, Expr::Lit {
            lit: yelang_lexer::Literal::Bool(true),
        });
    let _expr2 = expr(&mut hir, Expr::Lit {
            lit: yelang_lexer::Literal::Bool(false),
        });
    let left = _expr1;
    let right = _expr2;
    let _expr3 = expr(&mut hir, Expr::Binary {
            op: BinaryOp::And,
            left: left,
            right: right,
        });
    let mut fcx = mk_fcx(&interner, &hir);


    let e = _expr3;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, interner.mk_ty(TyKind::Bool));
}

#[test]
fn binary_bitwise_unifies_operands() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _expr1 = expr(&mut hir, Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(1),
                suffix: None,
            }),
        });
    let _expr2 = expr(&mut hir, Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(2),
                suffix: None,
            }),
        });
    let left = _expr1;
    let right = _expr2;
    let _expr3 = expr(&mut hir, Expr::Binary {
            op: BinaryOp::BitAnd,
            left: left,
            right: right,
        });
    let mut fcx = mk_fcx(&interner, &hir);


    let e = _expr3;
    let ty = check_expr(&mut fcx, e);
    assert!(matches!(
        ty.kind(),
        TyKind::Infer(yelang_ty::ty::InferTy::IntVar(_))
    ));
}

// ---------------------------------------------------------------------------
// Unary operator checking
// ---------------------------------------------------------------------------

#[test]
fn unary_neg_preserves_type() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _expr1 = expr(&mut hir, Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(1),
                suffix: None,
            }),
        });
    let inner = _expr1;
    let _expr2 = expr(&mut hir, Expr::Unary {
            op: UnaryOp::Neg,
            expr: inner,
        });
    let mut fcx = mk_fcx(&interner, &hir);

    let e = _expr2;
    let ty = check_expr(&mut fcx, e);
    assert!(matches!(
        ty.kind(),
        TyKind::Infer(yelang_ty::ty::InferTy::IntVar(_))
    ));
}

#[test]
fn unary_not_preserves_type() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _expr1 = expr(&mut hir, Expr::Lit {
            lit: yelang_lexer::Literal::Bool(true),
        });
    let inner = _expr1;
    let _expr2 = expr(&mut hir, Expr::Unary {
            op: UnaryOp::Not,
            expr: inner,
        });
    let mut fcx = mk_fcx(&interner, &hir);

    let e = _expr2;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, interner.mk_ty(TyKind::Bool));
}

#[test]
fn unary_deref_on_reference() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _pat1 = pat(&mut hir, Pat::Err);
    let _expr1 = expr(&mut hir, Expr::Path { res: local_res(_pat1) });
    let inner = _expr1;
    let _expr2 = expr(&mut hir, Expr::Unary {
            op: UnaryOp::Deref,
            expr: inner,
        });
    let mut fcx = mk_fcx(&interner, &hir);
    let pointee = interner.mk_ty(TyKind::Int(IntTy::I32));
    let ref_ty = interner.mk_ty(TyKind::Ref(pointee, Mutability::Not));
    fcx.insert_local(_pat1, ref_ty);

    let e = _expr2;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, pointee);
}

#[test]
fn unary_deref_inference() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _expr1 = expr(&mut hir, Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(1),
                suffix: None,
            }),
        });
    let inner = _expr1;
    let _expr2 = expr(&mut hir, Expr::Unary {
            op: UnaryOp::Deref,
            expr: inner,
        });
    let mut fcx = mk_fcx(&interner, &hir);

    let e = _expr2;
    let ty = check_expr(&mut fcx, e);
    // The operand is an int var, not a reference, so this is an error.
    assert_eq!(ty, interner.mk_ty(TyKind::Error));
}

#[test]
fn unary_ref_creates_reference() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _expr1 = expr(&mut hir, Expr::Lit {
            lit: yelang_lexer::Literal::Bool(true),
        });
    let inner = _expr1;
    let _expr2 = expr(&mut hir, Expr::Unary {
            op: UnaryOp::Ref,
            expr: inner,
        });
    let mut fcx = mk_fcx(&interner, &hir);

    let e = _expr2;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(
        ty,
        interner.mk_ty(TyKind::Ref(interner.mk_ty(TyKind::Bool), Mutability::Not))
    );
}

#[test]
fn unary_refmut_creates_mutable_reference() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _expr1 = expr(&mut hir, Expr::Lit {
            lit: yelang_lexer::Literal::Bool(true),
        });
    let inner = _expr1;
    let _expr2 = expr(&mut hir, Expr::Unary {
            op: UnaryOp::RefMut,
            expr: inner,
        });
    let mut fcx = mk_fcx(&interner, &hir);

    let e = _expr2;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(
        ty,
        interner.mk_ty(TyKind::Ref(interner.mk_ty(TyKind::Bool), Mutability::Mut))
    );
}

// ---------------------------------------------------------------------------
// Call checking
// ---------------------------------------------------------------------------

#[test]
fn call_fn_ptr() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _pat1 = pat(&mut hir, Pat::Err);
    let _expr1 = expr(&mut hir, Expr::Path { res: local_res(_pat1) });
    let _expr2 = expr(&mut hir, Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(1),
                suffix: None,
            }),
        });
    let func = _expr1;
    let arg = _expr2;
    let _expr3 = expr(&mut hir, Expr::Call {
            func: func,
            args: vec![arg],
        });
    let mut fcx = mk_fcx(&interner, &hir);
    let i32_ty = interner.mk_ty(TyKind::Int(IntTy::I32));
    let bool_ty = interner.mk_ty(TyKind::Bool);
    let inputs = interner.mk_generic_args(&[yelang_ty::generic::GenericArg::Type(i32_ty)]);
    let fn_ty = interner.mk_ty(TyKind::FnPtr(yelang_ty::ty::PolyFnSig {
        sig: yelang_ty::ty::FnSig {
            inputs,
            output: bool_ty,
        },
    }));
    fcx.insert_local(_pat1, fn_ty);


    let e = _expr3;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, bool_ty);
}

#[test]
fn call_wrong_arg_count_is_error() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _pat1 = pat(&mut hir, Pat::Err);
    let _expr1 = expr(&mut hir, Expr::Path { res: local_res(_pat1) });
    let func = _expr1;
    let _expr3 = expr(&mut hir, Expr::Call {
            func: func,
            args: vec![],
        });
    let mut fcx = mk_fcx(&interner, &hir);
    let i32_ty = interner.mk_ty(TyKind::Int(IntTy::I32));
    let inputs = interner.mk_generic_args(&[yelang_ty::generic::GenericArg::Type(i32_ty)]);
    let fn_ty = interner.mk_ty(TyKind::FnPtr(yelang_ty::ty::PolyFnSig {
        sig: yelang_ty::ty::FnSig {
            inputs,
            output: i32_ty,
        },
    }));
    fcx.insert_local(_pat1, fn_ty);

    let e = _expr3;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, interner.mk_ty(TyKind::Error));
}

#[test]
fn call_unknown_function_inference() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _pat1 = pat(&mut hir, Pat::Err);
    let _expr1 = expr(&mut hir, Expr::Path { res: local_res(_pat1) });
    let _expr2 = expr(&mut hir, Expr::Lit {
            lit: yelang_lexer::Literal::Bool(true),
        });
    let func = _expr1;
    let arg = _expr2;
    let _expr3 = expr(&mut hir, Expr::Call {
            func: func,
            args: vec![arg],
        });
    let mut fcx = mk_fcx(&interner, &hir);
    let func_ty = fcx.new_ty_var();
    fcx.insert_local(_pat1, func_ty);


    let e = _expr3;
    let ty = check_expr(&mut fcx, e);
    // Should infer a function type and return a type variable.
    assert!(matches!(
        ty.kind(),
        TyKind::Infer(yelang_ty::ty::InferTy::TyVar(_))
    ));
}

// ---------------------------------------------------------------------------
// Field access checking
// ---------------------------------------------------------------------------

#[test]
fn field_tuple_access() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _pat1 = pat(&mut hir, Pat::Err);
    let _expr1 = expr(&mut hir, Expr::Path { res: local_res(_pat1) });
    let base = _expr1;
    let _expr2 = expr(&mut hir, Expr::Field {
            expr: base,
            field: yelang_ast::Ident {
                symbol: symbol(0),
                span: dummy_span(),
                origin: yelang_ast::IdentOrigin::Plain,
            },
        });
    let mut fcx = mk_fcx(&interner, &hir);
    let i32_ty = interner.mk_ty(TyKind::Int(IntTy::I32));
    let bool_ty = interner.mk_ty(TyKind::Bool);
    let tuple_ty = interner.mk_ty(TyKind::Tuple(interner.mk_generic_args(&[
        yelang_ty::generic::GenericArg::Type(i32_ty),
        yelang_ty::generic::GenericArg::Type(bool_ty),
    ])));
    fcx.insert_local(_pat1, tuple_ty);

    let e = _expr2;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, i32_ty);
}

#[test]
fn field_tuple_out_of_bounds_is_error() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _pat1 = pat(&mut hir, Pat::Err);
    let _expr1 = expr(&mut hir, Expr::Path { res: local_res(_pat1) });
    let base = _expr1;
    let _expr2 = expr(&mut hir, Expr::Field {
            expr: base,
            field: yelang_ast::Ident {
                symbol: symbol(5),
                span: dummy_span(),
                origin: yelang_ast::IdentOrigin::Plain,
            },
        });
    let mut fcx = mk_fcx(&interner, &hir);
    let i32_ty = interner.mk_ty(TyKind::Int(IntTy::I32));
    let tuple_ty = interner.mk_ty(TyKind::Tuple(
        interner.mk_generic_args(&[yelang_ty::generic::GenericArg::Type(i32_ty)]),
    ));
    fcx.insert_local(_pat1, tuple_ty);

    let e = _expr2;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, interner.mk_ty(TyKind::Error));
}

// ---------------------------------------------------------------------------
// Index checking
// ---------------------------------------------------------------------------

#[test]
fn index_array() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _pat1 = pat(&mut hir, Pat::Err);
    let _expr1 = expr(&mut hir, Expr::Path { res: local_res(_pat1) });
    let _expr2 = expr(&mut hir, Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(1),
                suffix: None,
            }),
        });
    let base = _expr1;
    let idx = _expr2;
    let _expr3 = expr(&mut hir, Expr::Index {
            expr: base,
            index: idx,
        });
    let mut fcx = mk_fcx(&interner, &hir);
    let i32_ty = interner.mk_ty(TyKind::Int(IntTy::I32));
    let array_ty = interner.mk_ty(TyKind::Array(
        i32_ty,
        yelang_ty::ty::Const {
            kind: yelang_ty::ty::ConstKind::Value(yelang_ty::ty::ConstValue::Int(3)),
            ty: i32_ty,
        },
    ));
    fcx.insert_local(_pat1, array_ty);


    let e = _expr3;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, i32_ty);
}

#[test]
fn index_slice() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _pat1 = pat(&mut hir, Pat::Err);
    let _expr1 = expr(&mut hir, Expr::Path { res: local_res(_pat1) });
    let _expr2 = expr(&mut hir, Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(1),
                suffix: None,
            }),
        });
    let base = _expr1;
    let idx = _expr2;
    let _expr3 = expr(&mut hir, Expr::Index {
            expr: base,
            index: idx,
        });
    let mut fcx = mk_fcx(&interner, &hir);
    let i32_ty = interner.mk_ty(TyKind::Int(IntTy::I32));
    let slice_ty = interner.mk_ty(TyKind::Slice(i32_ty));
    fcx.insert_local(_pat1, slice_ty);


    let e = _expr3;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, i32_ty);
}

// ---------------------------------------------------------------------------
// Assignment checking
// ---------------------------------------------------------------------------

#[test]
fn assign_unifies_types() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _pat1 = pat(&mut hir, Pat::Err);
    let _expr1 = expr(&mut hir, Expr::Path { res: local_res(_pat1) });
    let _expr2 = expr(&mut hir, Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(1),
                suffix: None,
            }),
        });
    let left = _expr1;
    let right = _expr2;
    let _expr3 = expr(&mut hir, Expr::Assign {
            left: left,
            right: right,
        });
    let mut fcx = mk_fcx(&interner, &hir);
    let i32_ty = interner.mk_ty(TyKind::Int(IntTy::I32));
    fcx.insert_local(_pat1, i32_ty);


    let e = _expr3;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(
        ty,
        interner.mk_ty(TyKind::Tuple(yelang_ty::list::List::empty()))
    );
}

// ---------------------------------------------------------------------------
// Block checking
// ---------------------------------------------------------------------------

#[test]
fn block_empty_is_unit() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _block1 = block(&mut hir, vec![], None);
    let b = _block1;
    let _expr1 = expr(&mut hir, Expr::Block { block: b });
    let mut fcx = mk_fcx(&interner, &hir);

    let e = _expr1;
    let ty = check_expr(&mut fcx, e);
    assert!(ty.is_unit());
}

#[test]
fn block_with_trailing_expr() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _expr2 = expr(&mut hir, Expr::Lit {
            lit: yelang_lexer::Literal::Bool(true),
        });
    let trailing = _expr2;
    let _block1 = block(&mut hir, vec![], Some(trailing));
    let b = _block1;
    let _expr1 = expr(&mut hir, Expr::Block { block: b });
    let mut fcx = mk_fcx(&interner, &hir);


    let e = _expr1;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, interner.mk_ty(TyKind::Bool));
}

#[test]
fn block_with_let_binding() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _expr2 = expr(&mut hir, Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(1),
                suffix: None,
            }),
        });
    let _pat3 = pat(&mut hir, Pat::Binding {
            mode: BindingMode::ByValue,
            name: symbol(1),
            subpat: None,
        });
    let p = _pat3;
    let init = _expr2;
    let _stmt1 = stmt_let(&mut hir, p, Some(init));
    let s = _stmt1;
    let _block1 = block(&mut hir, vec![s], None);
    let b = _block1;
    let _expr1 = expr(&mut hir, Expr::Block { block: b });
    let mut fcx = mk_fcx(&interner, &hir);




    let e = _expr1;
    let ty = check_expr(&mut fcx, e);
    assert!(ty.is_unit());
}

// ---------------------------------------------------------------------------
// Loop and break checking
// ---------------------------------------------------------------------------

#[test]
fn loop_without_break_is_never() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _block1 = block(&mut hir, vec![], None);
    let b = _block1;
    let _expr1 = expr(&mut hir, Expr::Loop {
            block: b,
            label: None,
        });
    let mut fcx = mk_fcx(&interner, &hir);

    let e = _expr1;
    let ty = check_expr(&mut fcx, e);
    assert!(ty.is_never());
}

#[test]
fn loop_with_break_value() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _expr2 = expr(&mut hir, Expr::Lit {
                    lit: yelang_lexer::Literal::Bool(true),
                });
    let _expr3 = expr(&mut hir, Expr::Break {
            label: None,
            expr: Some(_expr2),
        });
    let break_expr = _expr3;
    let _stmt1 = stmt_expr(&mut hir, break_expr);
    let _block1 = block(&mut hir, vec![_stmt1], None);
    let b = _block1;
    let _expr1 = expr(&mut hir, Expr::Loop {
            block: b,
            label: None,
        });
    let mut fcx = mk_fcx(&interner, &hir);


    let e = _expr1;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, interner.mk_ty(TyKind::Bool));
}

#[test]
fn break_without_value_is_unit() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _expr2 = expr(&mut hir, Expr::Break {
            label: None,
            expr: None,
        });
    let break_expr = _expr2;
    let _stmt1 = stmt_expr(&mut hir, break_expr);
    let _block1 = block(&mut hir, vec![_stmt1], None);
    let b = _block1;
    let _expr1 = expr(&mut hir, Expr::Loop {
            block: b,
            label: None,
        });
    let mut fcx = mk_fcx(&interner, &hir);


    let e = _expr1;
    let ty = check_expr(&mut fcx, e);
    assert!(ty.is_unit());
}

#[test]
fn continue_is_never() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _expr2 = expr(&mut hir, Expr::Continue { label: None });
    let continue_expr = _expr2;
    let _stmt1 = stmt_expr(&mut hir, continue_expr);
    let _block1 = block(&mut hir, vec![_stmt1], None);
    let b = _block1;
    let _expr1 = expr(&mut hir, Expr::Loop {
            block: b,
            label: None,
        });
    let mut fcx = mk_fcx(&interner, &hir);


    let e = _expr1;
    let ty = check_expr(&mut fcx, e);
    assert!(ty.is_never());
}

// ---------------------------------------------------------------------------
// Return checking
// ---------------------------------------------------------------------------

#[test]
fn return_with_value_unifies_with_return_ty() {
    let interner = Interner::new();
    let i32_ty = interner.mk_ty(TyKind::Int(IntTy::I32));
    let mut hir = hir_crate();
    let _expr2 = expr(&mut hir, Expr::Lit {
                    lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                        value: symbol(1),
                        suffix: None,
                    }),
                });
    let _expr1 = expr(&mut hir, Expr::Return {
            expr: Some(_expr2),
        });
    let mut fcx = fcx_with_return_ty(&interner, &hir, i32_ty);
    let ret = _expr1;
    let ty = check_expr(&mut fcx, ret);
    assert!(ty.is_never());
}

#[test]
fn return_without_value_unifies_with_unit() {
    let interner = Interner::new();
    let unit_ty = interner.mk_ty(TyKind::Tuple(yelang_ty::list::List::empty()));
    let mut hir = hir_crate();
    let _expr1 = expr(&mut hir, Expr::Return { expr: None });
    let mut fcx = fcx_with_return_ty(&interner, &hir, unit_ty);
    let ret = _expr1;
    let ty = check_expr(&mut fcx, ret);
    assert!(ty.is_never());
}

// ---------------------------------------------------------------------------
// If checking
// ---------------------------------------------------------------------------

#[test]
fn if_else_branches_unify() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _expr1 = expr(&mut hir, Expr::Lit {
            lit: yelang_lexer::Literal::Bool(true),
        });
    let _expr2 = expr(&mut hir, Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(1),
                suffix: None,
            }),
        });
    let _expr3 = expr(&mut hir, Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(2),
                suffix: None,
            }),
        });
    let cond = _expr1;
    let then_branch = _expr2;
    let else_branch = _expr3;
    let _expr4 = expr(&mut hir, Expr::If {
            cond: cond,
            then_branch: then_branch,
            else_branch: Some(else_branch),
        });
    let mut fcx = mk_fcx(&interner, &hir);



    let e = _expr4;
    let ty = check_expr(&mut fcx, e);
    assert!(matches!(
        ty.kind(),
        TyKind::Infer(yelang_ty::ty::InferTy::IntVar(_))
    ));
}

#[test]
fn if_without_else_requires_unit() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _expr1 = expr(&mut hir, Expr::Lit {
            lit: yelang_lexer::Literal::Bool(true),
        });
    let _block1 = block(&mut hir, vec![], None);
    let _expr2 = expr(&mut hir, Expr::Block {
            block: _block1,
        });
    let cond = _expr1;
    let then_branch = _expr2;
    let _expr3 = expr(&mut hir, Expr::If {
            cond: cond,
            then_branch: then_branch,
            else_branch: None,
        });
    let mut fcx = mk_fcx(&interner, &hir);


    let e = _expr3;
    let ty = check_expr(&mut fcx, e);
    assert!(ty.is_unit());
}

// ---------------------------------------------------------------------------
// Match checking
// ---------------------------------------------------------------------------

#[test]
fn match_arms_unify() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _expr1 = expr(&mut hir, Expr::Lit {
            lit: yelang_lexer::Literal::Bool(true),
        });
    let _pat2 = pat(&mut hir, Pat::Wild);
    let _expr3 = expr(&mut hir, Expr::Lit {
                lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                    value: symbol(1),
                    suffix: None,
                }),
            });
    let _pat4 = pat(&mut hir, Pat::Wild);
    let _expr5 = expr(&mut hir, Expr::Lit {
                lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                    value: symbol(2),
                    suffix: None,
                }),
            });
    let scrutinee = _expr1;
    let arm1 = Arm {
        pat: _pat2,
        guard: None,
        body: _expr3,
        span: dummy_span(),
    };
    let arm2 = Arm {
        pat: _pat4,
        guard: None,
        body: _expr5,
        span: dummy_span(),
    };
    let _expr6 = expr(&mut hir, Expr::Match {
            expr: scrutinee,
            arms: vec![arm1, arm2],
        });
    let mut fcx = mk_fcx(&interner, &hir);



    let e = _expr6;
    let ty = check_expr(&mut fcx, e);
    // check_match returns a TyVar that gets unified with the arm types.
    // After resolution it should be an IntVar.
    let resolved = fcx.resolve_ty(ty);
    assert!(matches!(
        resolved.kind(),
        TyKind::Infer(yelang_ty::ty::InferTy::IntVar(_))
    ));
}

#[test]
fn match_guard_must_be_bool() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _expr1 = expr(&mut hir, Expr::Lit {
            lit: yelang_lexer::Literal::Bool(true),
        });
    let _pat2 = pat(&mut hir, Pat::Wild);
    let _expr3 = expr(&mut hir, Expr::Lit {
                lit: yelang_lexer::Literal::Bool(true),
            });
    let _expr4 = expr(&mut hir, Expr::Lit {
                lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                    value: symbol(1),
                    suffix: None,
                }),
            });
    let scrutinee = _expr1;
    let arm = Arm {
        pat: _pat2,
        guard: Some(_expr3),
        body: _expr4,
        span: dummy_span(),
    };
    let _expr5 = expr(&mut hir, Expr::Match {
            expr: scrutinee,
            arms: vec![arm],
        });
    let mut fcx = mk_fcx(&interner, &hir);


    let e = _expr5;
    let ty = check_expr(&mut fcx, e);
    let resolved = fcx.resolve_ty(ty);
    assert!(matches!(
        resolved.kind(),
        TyKind::Infer(yelang_ty::ty::InferTy::IntVar(_))
    ));
}

// ---------------------------------------------------------------------------
// Let expression checking (if-let)
// ---------------------------------------------------------------------------

#[test]
fn let_expr_returns_bool() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _expr1 = expr(&mut hir, Expr::Lit {
            lit: yelang_lexer::Literal::Bool(true),
        });
    let _pat2 = pat(&mut hir, Pat::Wild);
    let p = _pat2;
    let scrutinee = _expr1;
    let _expr3 = expr(&mut hir, Expr::Let {
            pat: p,
            expr: scrutinee,
        });
    let mut fcx = mk_fcx(&interner, &hir);


    let e = _expr3;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, interner.mk_ty(TyKind::Bool));
}

// ---------------------------------------------------------------------------
// Tuple checking
// ---------------------------------------------------------------------------

#[test]
fn tuple_literal() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _expr1 = expr(&mut hir, Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(1),
                suffix: None,
            }),
        });
    let _expr2 = expr(&mut hir, Expr::Lit {
            lit: yelang_lexer::Literal::Bool(true),
        });
    let a = _expr1;
    let b = _expr2;
    let _expr3 = expr(&mut hir, Expr::Tuple { exprs: vec![a, b] });
    let mut fcx = mk_fcx(&interner, &hir);


    let e = _expr3;
    let ty = check_expr(&mut fcx, e);
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
    let mut hir = hir_crate();
    let _expr1 = expr(&mut hir, Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(1),
                suffix: None,
            }),
        });
    let _expr2 = expr(&mut hir, Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(2),
                suffix: None,
            }),
        });
    let a = _expr1;
    let b = _expr2;
    let _expr3 = expr(&mut hir, Expr::Array { exprs: vec![a, b] });
    let mut fcx = mk_fcx(&interner, &hir);


    let e = _expr3;
    let ty = check_expr(&mut fcx, e);
    match ty.kind() {
        TyKind::Array(elem, _) => {
            assert!(matches!(
                elem.kind(),
                TyKind::Infer(yelang_ty::ty::InferTy::IntVar(_))
            ));
        }
        _ => panic!("expected array"),
    }
}

#[test]
fn array_literal_empty() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _expr1 = expr(&mut hir, Expr::Array { exprs: vec![] });
    let mut fcx = mk_fcx(&interner, &hir);
    let e = _expr1;
    let ty = check_expr(&mut fcx, e);
    match ty.kind() {
        TyKind::Array(_, len) => {
            assert!(matches!(
                len.kind,
                yelang_ty::ty::ConstKind::Value(yelang_ty::ty::ConstValue::Int(0))
            ));
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
    let mut hir = hir_crate();
    let _pat1 = pat(&mut hir, Pat::Err);
    let _expr2 = expr(&mut hir, Expr::Lit {
                lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                    value: symbol(1),
                    suffix: None,
                }),
            });
    let path = local_res(_pat1);
    let field = FieldExpr {
        ident: yelang_ast::Ident {
            symbol: symbol(1),
            span: dummy_span(),
            origin: yelang_ast::IdentOrigin::Plain,
        },
        expr: _expr2,
        span: dummy_span(),
    };
    let _expr3 = expr(&mut hir, Expr::Struct {
            path,
            fields: vec![field],
            rest: None,
        });
    let mut fcx = mk_fcx(&interner, &hir);
    let i32_ty = interner.mk_ty(TyKind::Int(IntTy::I32));
    fcx.insert_local(_pat1, i32_ty);


    let e = _expr3;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, i32_ty);
}

// ---------------------------------------------------------------------------
// Cast checking
// ---------------------------------------------------------------------------

#[test]
fn cast_returns_target_type() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _expr1 = expr(&mut hir, Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(1),
                suffix: None,
            }),
        });
    let target = HirTy::Path {
        res: Res::PrimTy {
            ty: yelang_hir::res::PrimTy::Int(yelang_hir::res::IntTy::I64),
        },
        args: vec![],
    };
    let _ty1 = hir_ty(&mut hir, target);
    let inner = _expr1;
    let _expr2 = expr(&mut hir, Expr::Cast {
            expr: inner,
            ty: _ty1,
        });
    let mut fcx = mk_fcx(&interner, &hir);


    let e = _expr2;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, interner.mk_ty(TyKind::Int(IntTy::I64)));
}

// ---------------------------------------------------------------------------
// Pattern checking
// ---------------------------------------------------------------------------

#[test]
fn pat_wild() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _pat1 = pat(&mut hir, Pat::Wild);
    let mut fcx = mk_fcx(&interner, &hir);
    let i32_ty = interner.mk_ty(TyKind::Int(IntTy::I32));
    let p = _pat1;
    check_pat(&mut fcx, p, i32_ty);
    assert_eq!(fcx.results.pat_ty(_pat1), Some(i32_ty));
}

#[test]
fn pat_binding_inserts_local() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _pat1 = pat(&mut hir, Pat::Binding {
            mode: BindingMode::ByValue,
            name: symbol(1),
            subpat: None,
        });
    let mut fcx = mk_fcx(&interner, &hir);
    let i32_ty = interner.mk_ty(TyKind::Int(IntTy::I32));
    let p = _pat1;
    check_pat(&mut fcx, p, i32_ty);
    assert_eq!(fcx.results.local_ty(_pat1), Some(i32_ty));
    assert_eq!(fcx.results.pat_ty(_pat1), Some(i32_ty));
}

#[test]
fn pat_tuple() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _pat2 = pat(&mut hir, Pat::Wild);
    let _pat3 = pat(&mut hir, Pat::Wild);
    let _pat1 = pat(&mut hir, Pat::Tuple {
            pats: vec![_pat2, _pat3],
        });
    let mut fcx = mk_fcx(&interner, &hir);
    let i32_ty = interner.mk_ty(TyKind::Int(IntTy::I32));
    let p = _pat1;
    check_pat(&mut fcx, p, i32_ty);
    assert_eq!(fcx.results.pat_ty(_pat1), Some(i32_ty));
}

#[test]
fn pat_or() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _pat2 = pat(&mut hir, Pat::Wild);
    let _pat3 = pat(&mut hir, Pat::Wild);
    let _pat1 = pat(&mut hir, Pat::Or {
            pats: vec![_pat2, _pat3],
        });
    let mut fcx = mk_fcx(&interner, &hir);
    let i32_ty = interner.mk_ty(TyKind::Int(IntTy::I32));
    let p = _pat1;
    check_pat(&mut fcx, p, i32_ty);
    assert_eq!(fcx.results.pat_ty(_pat1), Some(i32_ty));
}

#[test]
fn pat_slice() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _pat2 = pat(&mut hir, Pat::Wild);
    let _pat1 = pat(&mut hir, Pat::Slice {
            prefix: vec![_pat2],
            middle: None,
            suffix: vec![],
        });
    let mut fcx = mk_fcx(&interner, &hir);
    let i32_ty = interner.mk_ty(TyKind::Int(IntTy::I32));
    let p = _pat1;
    check_pat(&mut fcx, p, i32_ty);
    assert_eq!(fcx.results.pat_ty(_pat1), Some(i32_ty));
}

#[test]
fn pat_err_is_error() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _pat1 = pat(&mut hir, Pat::Err);
    let mut fcx = mk_fcx(&interner, &hir);
    let p = _pat1;
    check_pat(&mut fcx, p, interner.mk_ty(TyKind::Int(IntTy::I32)));
    assert_eq!(
        fcx.results.pat_ty(_pat1),
        Some(interner.mk_ty(TyKind::Error))
    );
}

// ---------------------------------------------------------------------------
// Coercion checking
// ---------------------------------------------------------------------------

#[test]
fn coerce_exact_match() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let mut fcx = mk_fcx(&interner, &hir);
    let i32_ty = interner.mk_ty(TyKind::Int(IntTy::I32));
    let result = fcx.coerce(i32_ty, i32_ty);
    assert_eq!(result, Ok(i32_ty));
}

#[test]
fn coerce_mismatch_fails() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let mut fcx = mk_fcx(&interner, &hir);
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
    let mut hir = hir_crate();
    let _expr1 = expr(&mut hir, Expr::Err);
    let mut fcx = mk_fcx(&interner, &hir);
    let ty_var = fcx.new_ty_var();
    let i32_ty = interner.mk_ty(TyKind::Int(IntTy::I32));
    fcx.eq(ty_var, i32_ty).unwrap();
    fcx.results.expr_types.insert(_expr1, ty_var);

    writeback_types(&mut fcx);
    let resolved = fcx.results.expr_ty(_expr1).unwrap();
    assert_eq!(resolved, i32_ty);
}

#[test]
fn writeback_int_fallback_to_i32() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _expr1 = expr(&mut hir, Expr::Err);
    let mut fcx = mk_fcx(&interner, &hir);
    let int_var = fcx.new_int_var();
    fcx.results.expr_types.insert(_expr1, int_var);

    writeback_types(&mut fcx);
    let resolved = fcx.results.expr_ty(_expr1).unwrap();
    assert_eq!(resolved, interner.mk_ty(TyKind::Int(IntTy::I32)));
}

#[test]
fn writeback_float_fallback_to_f64() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _expr1 = expr(&mut hir, Expr::Err);
    let mut fcx = mk_fcx(&interner, &hir);
    let float_var = fcx.new_float_var();
    fcx.results.expr_types.insert(_expr1, float_var);

    writeback_types(&mut fcx);
    let resolved = fcx.results.expr_ty(_expr1).unwrap();
    assert_eq!(resolved, interner.mk_ty(TyKind::Float(FloatTy::F64)));
}

#[test]
fn writeback_unresolved_ty_var_becomes_error() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _expr1 = expr(&mut hir, Expr::Err);
    let mut fcx = mk_fcx(&interner, &hir);
    let ty_var = fcx.new_ty_var();
    fcx.results.expr_types.insert(_expr1, ty_var);

    writeback_types(&mut fcx);
    let resolved = fcx.results.expr_ty(_expr1).unwrap();
    assert_eq!(resolved, interner.mk_ty(TyKind::Error));
}

// ---------------------------------------------------------------------------
// Body checking
// ---------------------------------------------------------------------------

#[test]
fn body_check_params_and_expr() {
    let interner = Interner::new();
    let i32_ty = interner.mk_ty(TyKind::Int(IntTy::I32));
    let mut hir = hir_crate();
    let _pat1 = pat(&mut hir, Pat::Binding {
                mode: BindingMode::ByValue,
                name: symbol(1),
                subpat: None,
            });
    let _ty1 = hir_ty(&mut hir, HirTy::Path {
                res: Res::PrimTy {
                    ty: yelang_hir::res::PrimTy::Int(yelang_hir::res::IntTy::I32),
                },
                args: vec![],
            },);
    let _expr2 = expr(&mut hir, Expr::Path { res: local_res(_pat1) });
    let param = yelang_hir::hir_body::Param {
        pat: _pat1,
        ty: _ty1,
        span: dummy_span(),
    };
    let _body1 = body(&mut hir, vec![param], _expr2);
    let mut fcx = fcx_with_return_ty(&interner, &hir, i32_ty);

    let body_id = _body1;
    check_body(&mut fcx, body_id);
    assert_eq!(fcx.results.local_ty(_pat1), Some(i32_ty));
}

// ---------------------------------------------------------------------------
// Error recovery
// ---------------------------------------------------------------------------

#[test]
fn expr_err_is_error() {
    let interner = Interner::new();
    let mut hir = hir_crate();
    let _expr1 = expr(&mut hir, Expr::Err);
    let mut fcx = mk_fcx(&interner, &hir);
    let e = _expr1;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, interner.mk_ty(TyKind::Error));
}
