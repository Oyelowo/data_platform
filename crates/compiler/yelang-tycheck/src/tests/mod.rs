/*! Exhaustive tests for yelang-tycheck.
 *
 * Covers every ExprKind, every PatKind, coercion cases, writeback,
 * and error recovery paths.
 */

#![allow(unused_mut)]

use yelang_arena::DefId;
use yelang_ast::{BinaryOp, UnaryOp};
use yelang_hir as hir;
use yelang_hir::Crate as HirCrate;
use yelang_hir::hir::body::{Body, Param};
use yelang_hir::hir::core::{Arm, Block, Expr, FieldExpr, Stmt};
use yelang_hir::hir::pat::{BindingMode, Pat};
use yelang_hir::ids::{BodyId, ExprId, HirTyId, PatId, StmtId};
use yelang_hir::res::Res;
use yelang_interner::Symbol;
use yelang_lexer::{Position, Span};
use yelang_ty::primitive::{FloatTy, IntTy};
use yelang_ty::ty::{AnonField, AnonStructDef, Mutability, Ty, TyId};

use crate::autoderef::Adjustment;
use crate::check::{check_body, check_expr};
use crate::coerce::Coerce;
use crate::collector::collect_crate_types;
use crate::fn_ctxt::FnCtxt;
use crate::hir_ty_lower::lower_hir_ty_id;
use crate::pat::check_pat;
use crate::tcx::{BuiltinTraitKind, TyCtxt};
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
    hir.alloc_stmt(
        Stmt::Let {
            pat,
            ty: None,
            init,
        },
        dummy_span(),
    )
}

fn body(hir: &mut HirCrate, params: Vec<Param>, value: ExprId) -> BodyId {
    hir.alloc_body(
        Body {
            params,
            value,
            span: dummy_span(),
        },
        dummy_span(),
    )
}

fn hir_ty(hir: &mut HirCrate, ty: hir::Ty) -> HirTyId {
    hir.alloc_ty(ty, dummy_span())
}

fn hir_i32_id(hir: &mut HirCrate) -> HirTyId {
    hir_ty(
        hir,
        hir::Ty::Path {
            res: Res::PrimTy {
                ty: yelang_hir::res::PrimTy::Int(yelang_hir::res::IntTy::I32),
            },
            args: vec![],
        },
    )
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

fn fcx_with_return_ty<'a>(tcx: &'a TyCtxt, return_ty: TyId) -> FnCtxt<'a> {
    FnCtxt::new(tcx, def_id(1), return_ty)
}

fn mk_fcx<'a>(tcx: &'a TyCtxt) -> FnCtxt<'a> {
    let unit = tcx
        .interner()
        .mk_ty(Ty::Tuple(yelang_ty::list::List::empty()));
    fcx_with_return_ty(tcx, unit)
}

// ---------------------------------------------------------------------------
// Literal checking
// ---------------------------------------------------------------------------

#[test]
fn literal_int_creates_int_var() {
    let mut hir = hir_crate();
    let _expr1 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(1),
                suffix: None,
            }),
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);
    let e = _expr1;
    let ty = check_expr(&mut fcx, e);
    assert!(matches!(
        tcx.interner().ty(ty),
        Ty::Infer(yelang_ty::ty::InferTy::IntVar(_))
    ));
}

#[test]
fn literal_float_creates_float_var() {
    let mut hir = hir_crate();
    let _expr1 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Float(yelang_lexer::FloatLit {
                value: symbol(1),
                suffix: None,
            }),
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);
    let e = _expr1;
    let ty = check_expr(&mut fcx, e);
    assert!(matches!(
        tcx.interner().ty(ty),
        Ty::Infer(yelang_ty::ty::InferTy::FloatVar(_))
    ));
}

#[test]
fn literal_bool_is_bool() {
    let mut hir = hir_crate();
    let _expr1 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Bool(true),
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);
    let e = _expr1;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, tcx.interner().mk_ty(Ty::Bool));
}

#[test]
fn literal_char_is_char() {
    let mut hir = hir_crate();
    let _expr1 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Char('a'),
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);
    let e = _expr1;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, tcx.interner().mk_ty(Ty::Char));
}

#[test]
fn literal_str_is_str() {
    let mut hir = hir_crate();
    let _expr1 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Str(yelang_lexer::StringLit {
                value: symbol(1),
                kind: yelang_lexer::StrKind::Normal,
            }),
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);
    let e = _expr1;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, tcx.interner().mk_ty(Ty::Str));
}

// ---------------------------------------------------------------------------
// Path checking
// ---------------------------------------------------------------------------

#[test]
fn path_local_lookup() {
    let mut hir = hir_crate();
    let _pat1 = pat(&mut hir, Pat::Err);
    let _expr2 = expr(
        &mut hir,
        Expr::Path {
            res: local_res(_pat1),
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);
    let local_ty = tcx.interner().mk_ty(Ty::Int(IntTy::I32));
    fcx.insert_local(_pat1, local_ty);

    let e = _expr2;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, local_ty);
}

#[test]
fn path_def_lookup() {
    let mut hir = hir_crate();
    let _expr2 = expr(&mut hir, Expr::Path { res: def_res(1) });

    let mut tcx = TyCtxt::new(hir);
    let def_ty = tcx.interner().mk_ty(Ty::Int(IntTy::I64));
    tcx.item_types.insert(def_id(1), def_ty);
    let mut fcx = FnCtxt::new(
        &tcx,
        def_id(1),
        tcx.interner()
            .mk_ty(Ty::Tuple(yelang_ty::list::List::empty())),
    );

    let e = _expr2;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, def_ty);
}

#[test]
fn path_missing_local_is_error() {
    let mut hir = hir_crate();
    let _pat99 = pat(&mut hir, Pat::Err);
    let _expr1 = expr(
        &mut hir,
        Expr::Path {
            res: local_res(_pat99),
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);
    let e = _expr1;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, tcx.interner().mk_ty(Ty::Error));
}

// ---------------------------------------------------------------------------
// Binary operator checking
// ---------------------------------------------------------------------------

#[test]
fn binary_arithmetic_unifies_operands() {
    let mut hir = hir_crate();
    let _expr1 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(1),
                suffix: None,
            }),
        },
    );
    let _expr2 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(2),
                suffix: None,
            }),
        },
    );
    let left = _expr1;
    let right = _expr2;
    let _expr3 = expr(
        &mut hir,
        Expr::Binary {
            op: BinaryOp::Add,
            left: left,
            right: right,
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);

    let e = _expr3;
    let ty = check_expr(&mut fcx, e);
    // Both operands are int vars; they unify, result is same int var.
    assert!(matches!(
        tcx.interner().ty(ty),
        Ty::Infer(yelang_ty::ty::InferTy::IntVar(_))
    ));
}

#[test]
fn binary_comparison_returns_bool() {
    let mut hir = hir_crate();
    let _expr1 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(1),
                suffix: None,
            }),
        },
    );
    let _expr2 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(2),
                suffix: None,
            }),
        },
    );
    let left = _expr1;
    let right = _expr2;
    let _expr3 = expr(
        &mut hir,
        Expr::Binary {
            op: BinaryOp::Eq,
            left: left,
            right: right,
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);

    let e = _expr3;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, tcx.interner().mk_ty(Ty::Bool));
}

#[test]
fn binary_logical_requires_bool() {
    let mut hir = hir_crate();
    let _expr1 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Bool(true),
        },
    );
    let _expr2 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Bool(false),
        },
    );
    let left = _expr1;
    let right = _expr2;
    let _expr3 = expr(
        &mut hir,
        Expr::Binary {
            op: BinaryOp::And,
            left: left,
            right: right,
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);

    let e = _expr3;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, tcx.interner().mk_ty(Ty::Bool));
}

#[test]
fn binary_bitwise_unifies_operands() {
    let mut hir = hir_crate();
    let _expr1 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(1),
                suffix: None,
            }),
        },
    );
    let _expr2 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(2),
                suffix: None,
            }),
        },
    );
    let left = _expr1;
    let right = _expr2;
    let _expr3 = expr(
        &mut hir,
        Expr::Binary {
            op: BinaryOp::BitAnd,
            left: left,
            right: right,
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);

    let e = _expr3;
    let ty = check_expr(&mut fcx, e);
    assert!(matches!(
        tcx.interner().ty(ty),
        Ty::Infer(yelang_ty::ty::InferTy::IntVar(_))
    ));
}

// ---------------------------------------------------------------------------
// Unary operator checking
// ---------------------------------------------------------------------------

#[test]
fn unary_neg_preserves_type() {
    let mut hir = hir_crate();
    let _expr1 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(1),
                suffix: None,
            }),
        },
    );
    let inner = _expr1;
    let _expr2 = expr(
        &mut hir,
        Expr::Unary {
            op: UnaryOp::Neg,
            expr: inner,
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);

    let e = _expr2;
    let ty = check_expr(&mut fcx, e);
    assert!(matches!(
        tcx.interner().ty(ty),
        Ty::Infer(yelang_ty::ty::InferTy::IntVar(_))
    ));
}

#[test]
fn unary_not_preserves_type() {
    let mut hir = hir_crate();
    let _expr1 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Bool(true),
        },
    );
    let inner = _expr1;
    let _expr2 = expr(
        &mut hir,
        Expr::Unary {
            op: UnaryOp::Not,
            expr: inner,
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);

    let e = _expr2;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, tcx.interner().mk_ty(Ty::Bool));
}

#[test]
fn unary_deref_on_reference() {
    let mut hir = hir_crate();
    let _pat1 = pat(&mut hir, Pat::Err);
    let _expr1 = expr(
        &mut hir,
        Expr::Path {
            res: local_res(_pat1),
        },
    );
    let inner = _expr1;
    let _expr2 = expr(
        &mut hir,
        Expr::Unary {
            op: UnaryOp::Deref,
            expr: inner,
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);
    let pointee = tcx.interner().mk_ty(Ty::Int(IntTy::I32));
    let ref_ty = tcx.interner().mk_ty(Ty::Ref(pointee, Mutability::Not));
    fcx.insert_local(_pat1, ref_ty);

    let e = _expr2;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, pointee);
}

#[test]
fn unary_deref_inference() {
    let mut hir = hir_crate();
    let _expr1 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(1),
                suffix: None,
            }),
        },
    );
    let inner = _expr1;
    let _expr2 = expr(
        &mut hir,
        Expr::Unary {
            op: UnaryOp::Deref,
            expr: inner,
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);

    let e = _expr2;
    let ty = check_expr(&mut fcx, e);
    // The operand is an int var, not a reference, so this is an error.
    assert_eq!(ty, tcx.interner().mk_ty(Ty::Error));
}

#[test]
fn unary_ref_creates_reference() {
    let mut hir = hir_crate();
    let _expr1 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Bool(true),
        },
    );
    let inner = _expr1;
    let _expr2 = expr(
        &mut hir,
        Expr::Unary {
            op: UnaryOp::Ref,
            expr: inner,
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);

    let e = _expr2;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(
        ty,
        tcx.interner()
            .mk_ty(Ty::Ref(tcx.interner().mk_ty(Ty::Bool), Mutability::Not))
    );
}

#[test]
fn unary_refmut_creates_mutable_reference() {
    let mut hir = hir_crate();
    let _expr1 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Bool(true),
        },
    );
    let inner = _expr1;
    let _expr2 = expr(
        &mut hir,
        Expr::Unary {
            op: UnaryOp::RefMut,
            expr: inner,
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);

    let e = _expr2;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(
        ty,
        tcx.interner()
            .mk_ty(Ty::Ref(tcx.interner().mk_ty(Ty::Bool), Mutability::Mut))
    );
}

// ---------------------------------------------------------------------------
// Call checking
// ---------------------------------------------------------------------------

#[test]
fn call_fn_ptr() {
    let mut hir = hir_crate();
    let _pat1 = pat(&mut hir, Pat::Err);
    let _expr1 = expr(
        &mut hir,
        Expr::Path {
            res: local_res(_pat1),
        },
    );
    let _expr2 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(1),
                suffix: None,
            }),
        },
    );
    let func = _expr1;
    let arg = _expr2;
    let _expr3 = expr(
        &mut hir,
        Expr::Call {
            func: func,
            args: vec![arg],
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);
    let i32_ty = tcx.interner().mk_ty(Ty::Int(IntTy::I32));
    let bool_ty = tcx.interner().mk_ty(Ty::Bool);
    let inputs = tcx
        .interner()
        .mk_generic_args(&[yelang_ty::generic::GenericArg::Type(i32_ty)]);
    let fn_ty = tcx.interner().mk_ty(Ty::FnPtr(yelang_ty::ty::PolyFnSig {
        sig: yelang_ty::ty::FnSig {
            inputs,
            output: bool_ty,
            return_ty_infer: false,
        },
    }));
    fcx.insert_local(_pat1, fn_ty);

    let e = _expr3;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, bool_ty);
}

#[test]
fn call_wrong_arg_count_is_error() {
    let mut hir = hir_crate();
    let _pat1 = pat(&mut hir, Pat::Err);
    let _expr1 = expr(
        &mut hir,
        Expr::Path {
            res: local_res(_pat1),
        },
    );
    let func = _expr1;
    let _expr3 = expr(
        &mut hir,
        Expr::Call {
            func: func,
            args: vec![],
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);
    let i32_ty = tcx.interner().mk_ty(Ty::Int(IntTy::I32));
    let inputs = tcx
        .interner()
        .mk_generic_args(&[yelang_ty::generic::GenericArg::Type(i32_ty)]);
    let fn_ty = tcx.interner().mk_ty(Ty::FnPtr(yelang_ty::ty::PolyFnSig {
        sig: yelang_ty::ty::FnSig {
            inputs,
            output: i32_ty,
            return_ty_infer: false,
        },
    }));
    fcx.insert_local(_pat1, fn_ty);

    let e = _expr3;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, tcx.interner().mk_ty(Ty::Error));
}

#[test]
fn call_unknown_function_inference() {
    let mut hir = hir_crate();
    let _pat1 = pat(&mut hir, Pat::Err);
    let _expr1 = expr(
        &mut hir,
        Expr::Path {
            res: local_res(_pat1),
        },
    );
    let _expr2 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Bool(true),
        },
    );
    let func = _expr1;
    let arg = _expr2;
    let _expr3 = expr(
        &mut hir,
        Expr::Call {
            func: func,
            args: vec![arg],
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);
    let func_ty = fcx.new_ty_var();
    fcx.insert_local(_pat1, func_ty);

    let e = _expr3;
    let ty = check_expr(&mut fcx, e);
    // Should infer a function type and return a type variable.
    assert!(matches!(
        tcx.interner().ty(ty),
        Ty::Infer(yelang_ty::ty::InferTy::TyVar(_))
    ));
}

// ---------------------------------------------------------------------------
// Field access checking
// ---------------------------------------------------------------------------

#[test]
fn field_tuple_access() {
    let mut hir = hir_crate();
    let _pat1 = pat(&mut hir, Pat::Err);
    let _expr1 = expr(
        &mut hir,
        Expr::Path {
            res: local_res(_pat1),
        },
    );
    let base = _expr1;
    let _expr2 = expr(
        &mut hir,
        Expr::Field {
            expr: base,
            field: yelang_ast::Ident {
                symbol: symbol(0),
                span: dummy_span(),
                origin: yelang_ast::IdentOrigin::Plain,
            },
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);
    let i32_ty = tcx.interner().mk_ty(Ty::Int(IntTy::I32));
    let bool_ty = tcx.interner().mk_ty(Ty::Bool);
    let tuple_ty = tcx
        .interner()
        .mk_ty(Ty::Tuple(tcx.interner().mk_generic_args(&[
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
    let mut hir = hir_crate();
    let _pat1 = pat(&mut hir, Pat::Err);
    let _expr1 = expr(
        &mut hir,
        Expr::Path {
            res: local_res(_pat1),
        },
    );
    let base = _expr1;
    let _expr2 = expr(
        &mut hir,
        Expr::Field {
            expr: base,
            field: yelang_ast::Ident {
                symbol: symbol(5),
                span: dummy_span(),
                origin: yelang_ast::IdentOrigin::Plain,
            },
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);
    let i32_ty = tcx.interner().mk_ty(Ty::Int(IntTy::I32));
    let tuple_ty = tcx
        .interner()
        .mk_ty(Ty::Tuple(tcx.interner().mk_generic_args(&[
            yelang_ty::generic::GenericArg::Type(i32_ty),
        ])));
    fcx.insert_local(_pat1, tuple_ty);

    let e = _expr2;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, tcx.interner().mk_ty(Ty::Error));
}

// ---------------------------------------------------------------------------
// Index checking
// ---------------------------------------------------------------------------

#[test]
fn index_array() {
    let mut hir = hir_crate();
    let _pat1 = pat(&mut hir, Pat::Err);
    let _expr1 = expr(
        &mut hir,
        Expr::Path {
            res: local_res(_pat1),
        },
    );
    let _expr2 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(1),
                suffix: None,
            }),
        },
    );
    let base = _expr1;
    let idx = _expr2;
    let _expr3 = expr(
        &mut hir,
        Expr::Index {
            expr: base,
            index: idx,
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);
    let i32_ty = tcx.interner().mk_ty(Ty::Int(IntTy::I32));
    let array_ty = tcx.interner().mk_ty(Ty::Array(
        i32_ty,
        tcx.interner().mk_const_from_parts(
            yelang_ty::ty::Const::Value(yelang_ty::ty::ConstValue::Int(3)),
            i32_ty,
        ),
    ));
    fcx.insert_local(_pat1, array_ty);

    let e = _expr3;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, i32_ty);
}

#[test]
fn index_slice() {
    let mut hir = hir_crate();
    let _pat1 = pat(&mut hir, Pat::Err);
    let _expr1 = expr(
        &mut hir,
        Expr::Path {
            res: local_res(_pat1),
        },
    );
    let _expr2 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(1),
                suffix: None,
            }),
        },
    );
    let base = _expr1;
    let idx = _expr2;
    let _expr3 = expr(
        &mut hir,
        Expr::Index {
            expr: base,
            index: idx,
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);
    let i32_ty = tcx.interner().mk_ty(Ty::Int(IntTy::I32));
    let slice_ty = tcx.interner().mk_ty(Ty::Slice(i32_ty));
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
    let mut hir = hir_crate();
    let _pat1 = pat(&mut hir, Pat::Err);
    let _expr1 = expr(
        &mut hir,
        Expr::Path {
            res: local_res(_pat1),
        },
    );
    let _expr2 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(1),
                suffix: None,
            }),
        },
    );
    let left = _expr1;
    let right = _expr2;
    let _expr3 = expr(
        &mut hir,
        Expr::Assign {
            left: left,
            right: right,
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);
    let i32_ty = tcx.interner().mk_ty(Ty::Int(IntTy::I32));
    fcx.insert_local(_pat1, i32_ty);

    let e = _expr3;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(
        ty,
        tcx.interner()
            .mk_ty(Ty::Tuple(yelang_ty::list::List::empty()))
    );
}

// ---------------------------------------------------------------------------
// Block checking
// ---------------------------------------------------------------------------

#[test]
fn block_empty_is_unit() {
    let mut hir = hir_crate();
    let _block1 = block(&mut hir, vec![], None);
    let b = _block1;
    let _expr1 = expr(&mut hir, Expr::Block { block: b });
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);

    let e = _expr1;
    let ty = check_expr(&mut fcx, e);
    assert!(tcx.interner().ty(ty).is_unit());
}

#[test]
fn block_with_trailing_expr() {
    let mut hir = hir_crate();
    let _expr2 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Bool(true),
        },
    );
    let trailing = _expr2;
    let _block1 = block(&mut hir, vec![], Some(trailing));
    let b = _block1;
    let _expr1 = expr(&mut hir, Expr::Block { block: b });
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);

    let e = _expr1;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, tcx.interner().mk_ty(Ty::Bool));
}

#[test]
fn block_with_let_binding() {
    let mut hir = hir_crate();
    let _expr2 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(1),
                suffix: None,
            }),
        },
    );
    let _pat3 = pat(
        &mut hir,
        Pat::Binding {
            mode: BindingMode::ByValue,
            name: symbol(1),
            subpat: None,
        },
    );
    let p = _pat3;
    let init = _expr2;
    let _stmt1 = stmt_let(&mut hir, p, Some(init));
    let s = _stmt1;
    let _block1 = block(&mut hir, vec![s], None);
    let b = _block1;
    let _expr1 = expr(&mut hir, Expr::Block { block: b });
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);

    let e = _expr1;
    let ty = check_expr(&mut fcx, e);
    assert!(tcx.interner().ty(ty).is_unit());
}

// ---------------------------------------------------------------------------
// Loop and break checking
// ---------------------------------------------------------------------------

#[test]
fn loop_without_break_is_never() {
    let mut hir = hir_crate();
    let _block1 = block(&mut hir, vec![], None);
    let b = _block1;
    let _expr1 = expr(
        &mut hir,
        Expr::Loop {
            block: b,
            label: None,
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);

    let e = _expr1;
    let ty = check_expr(&mut fcx, e);
    assert!(tcx.interner().ty(ty).is_never());
}

#[test]
fn loop_with_break_value() {
    let mut hir = hir_crate();
    let _expr2 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Bool(true),
        },
    );
    let _expr3 = expr(
        &mut hir,
        Expr::Break {
            label: None,
            expr: Some(_expr2),
        },
    );
    let break_expr = _expr3;
    let _stmt1 = stmt_expr(&mut hir, break_expr);
    let _block1 = block(&mut hir, vec![_stmt1], None);
    let b = _block1;
    let _expr1 = expr(
        &mut hir,
        Expr::Loop {
            block: b,
            label: None,
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);

    let e = _expr1;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, tcx.interner().mk_ty(Ty::Bool));
}

#[test]
fn break_without_value_is_unit() {
    let mut hir = hir_crate();
    let _expr2 = expr(
        &mut hir,
        Expr::Break {
            label: None,
            expr: None,
        },
    );
    let break_expr = _expr2;
    let _stmt1 = stmt_expr(&mut hir, break_expr);
    let _block1 = block(&mut hir, vec![_stmt1], None);
    let b = _block1;
    let _expr1 = expr(
        &mut hir,
        Expr::Loop {
            block: b,
            label: None,
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);

    let e = _expr1;
    let ty = check_expr(&mut fcx, e);
    assert!(tcx.interner().ty(ty).is_unit());
}

#[test]
fn continue_is_never() {
    let mut hir = hir_crate();
    let _expr2 = expr(&mut hir, Expr::Continue { label: None });
    let continue_expr = _expr2;
    let _stmt1 = stmt_expr(&mut hir, continue_expr);
    let _block1 = block(&mut hir, vec![_stmt1], None);
    let b = _block1;
    let _expr1 = expr(
        &mut hir,
        Expr::Loop {
            block: b,
            label: None,
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);

    let e = _expr1;
    let ty = check_expr(&mut fcx, e);
    assert!(tcx.interner().ty(ty).is_never());
}

// ---------------------------------------------------------------------------
// Return checking
// ---------------------------------------------------------------------------

#[test]
fn return_with_value_unifies_with_return_ty() {
    let mut hir = hir_crate();
    let _expr2 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(1),
                suffix: None,
            }),
        },
    );
    let _expr1 = expr(&mut hir, Expr::Return { expr: Some(_expr2) });
    let tcx = TyCtxt::new(hir);
    let i32_ty = tcx.interner().mk_ty(Ty::Int(IntTy::I32));
    let mut fcx = fcx_with_return_ty(&tcx, i32_ty);
    let ret = _expr1;
    let ty = check_expr(&mut fcx, ret);
    assert!(tcx.interner().ty(ty).is_never());
}

#[test]
fn return_without_value_unifies_with_unit() {
    let mut hir = hir_crate();
    let _expr1 = expr(&mut hir, Expr::Return { expr: None });
    let tcx = TyCtxt::new(hir);
    let unit_ty = tcx
        .interner()
        .mk_ty(Ty::Tuple(yelang_ty::list::List::empty()));
    let mut fcx = fcx_with_return_ty(&tcx, unit_ty);
    let ret = _expr1;
    let ty = check_expr(&mut fcx, ret);
    assert!(tcx.interner().ty(ty).is_never());
}

// ---------------------------------------------------------------------------
// If checking
// ---------------------------------------------------------------------------

#[test]
fn if_else_branches_unify() {
    let mut hir = hir_crate();
    let _expr1 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Bool(true),
        },
    );
    let _expr2 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(1),
                suffix: None,
            }),
        },
    );
    let _expr3 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(2),
                suffix: None,
            }),
        },
    );
    let cond = _expr1;
    let then_branch = _expr2;
    let else_branch = _expr3;
    let _expr4 = expr(
        &mut hir,
        Expr::If {
            cond: cond,
            then_branch: then_branch,
            else_branch: Some(else_branch),
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);

    let e = _expr4;
    let ty = check_expr(&mut fcx, e);
    assert!(matches!(
        tcx.interner().ty(ty),
        Ty::Infer(yelang_ty::ty::InferTy::IntVar(_))
    ));
}

#[test]
fn if_without_else_requires_unit() {
    let mut hir = hir_crate();
    let _expr1 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Bool(true),
        },
    );
    let _block1 = block(&mut hir, vec![], None);
    let _expr2 = expr(&mut hir, Expr::Block { block: _block1 });
    let cond = _expr1;
    let then_branch = _expr2;
    let _expr3 = expr(
        &mut hir,
        Expr::If {
            cond: cond,
            then_branch: then_branch,
            else_branch: None,
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);

    let e = _expr3;
    let ty = check_expr(&mut fcx, e);
    assert!(tcx.interner().ty(ty).is_unit());
}

// ---------------------------------------------------------------------------
// Match checking
// ---------------------------------------------------------------------------

#[test]
fn match_arms_unify() {
    let mut hir = hir_crate();
    let _expr1 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Bool(true),
        },
    );
    let _pat2 = pat(&mut hir, Pat::Wild);
    let _expr3 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(1),
                suffix: None,
            }),
        },
    );
    let _pat4 = pat(&mut hir, Pat::Wild);
    let _expr5 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(2),
                suffix: None,
            }),
        },
    );
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
    let _expr6 = expr(
        &mut hir,
        Expr::Match {
            expr: scrutinee,
            arms: vec![arm1, arm2],
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);

    let e = _expr6;
    let ty = check_expr(&mut fcx, e);
    // check_match returns a TyVar that gets unified with the arm types.
    // After resolution it should be an IntVar.
    let resolved = fcx.resolve_ty(ty);
    assert!(matches!(
        tcx.interner().ty(resolved),
        Ty::Infer(yelang_ty::ty::InferTy::IntVar(_))
    ));
}

#[test]
fn match_guard_must_be_bool() {
    let mut hir = hir_crate();
    let _expr1 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Bool(true),
        },
    );
    let _pat2 = pat(&mut hir, Pat::Wild);
    let _expr3 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Bool(true),
        },
    );
    let _expr4 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(1),
                suffix: None,
            }),
        },
    );
    let scrutinee = _expr1;
    let arm = Arm {
        pat: _pat2,
        guard: Some(_expr3),
        body: _expr4,
        span: dummy_span(),
    };
    let _expr5 = expr(
        &mut hir,
        Expr::Match {
            expr: scrutinee,
            arms: vec![arm],
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);

    let e = _expr5;
    let ty = check_expr(&mut fcx, e);
    let resolved = fcx.resolve_ty(ty);
    assert!(matches!(
        tcx.interner().ty(resolved),
        Ty::Infer(yelang_ty::ty::InferTy::IntVar(_))
    ));
}

// ---------------------------------------------------------------------------
// Let expression checking (if-let)
// ---------------------------------------------------------------------------

#[test]
fn let_expr_returns_bool() {
    let mut hir = hir_crate();
    let _expr1 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Bool(true),
        },
    );
    let _pat2 = pat(&mut hir, Pat::Wild);
    let p = _pat2;
    let scrutinee = _expr1;
    let _expr3 = expr(
        &mut hir,
        Expr::Let {
            pat: p,
            expr: scrutinee,
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);

    let e = _expr3;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, tcx.interner().mk_ty(Ty::Bool));
}

// ---------------------------------------------------------------------------
// Tuple checking
// ---------------------------------------------------------------------------

#[test]
fn tuple_literal() {
    let mut hir = hir_crate();
    let _expr1 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(1),
                suffix: None,
            }),
        },
    );
    let _expr2 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Bool(true),
        },
    );
    let a = _expr1;
    let b = _expr2;
    let _expr3 = expr(&mut hir, Expr::Tuple { exprs: vec![a, b] });
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);

    let e = _expr3;
    let ty = check_expr(&mut fcx, e);
    match tcx.interner().ty(ty) {
        Ty::Tuple(args) => {
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
    let mut hir = hir_crate();
    let _expr1 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(1),
                suffix: None,
            }),
        },
    );
    let _expr2 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(2),
                suffix: None,
            }),
        },
    );
    let a = _expr1;
    let b = _expr2;
    let _expr3 = expr(&mut hir, Expr::Array { exprs: vec![a, b] });
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);

    let e = _expr3;
    let ty = check_expr(&mut fcx, e);
    match tcx.interner().ty(ty) {
        Ty::Array(elem, _) => {
            assert!(matches!(
                tcx.interner().ty(elem),
                Ty::Infer(yelang_ty::ty::InferTy::IntVar(_))
            ));
        }
        _ => panic!("expected array"),
    }
}

#[test]
fn array_literal_empty() {
    let mut hir = hir_crate();
    let _expr1 = expr(&mut hir, Expr::Array { exprs: vec![] });
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);
    let e = _expr1;
    let ty = check_expr(&mut fcx, e);
    match tcx.interner().ty(ty) {
        Ty::Array(_, len) => {
            assert!(matches!(
                tcx.interner().const_kind(len),
                yelang_ty::ty::Const::Value(yelang_ty::ty::ConstValue::Int(0))
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
    let mut hir = hir_crate();
    let _pat1 = pat(&mut hir, Pat::Err);
    let _expr2 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(1),
                suffix: None,
            }),
        },
    );
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
    let _expr3 = expr(
        &mut hir,
        Expr::Struct {
            path,
            fields: vec![field],
            rest: None,
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);
    let i32_ty = tcx.interner().mk_ty(Ty::Int(IntTy::I32));
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
    let mut hir = hir_crate();
    let _expr1 = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(1),
                suffix: None,
            }),
        },
    );
    let target = hir::Ty::Path {
        res: Res::PrimTy {
            ty: yelang_hir::res::PrimTy::Int(yelang_hir::res::IntTy::I64),
        },
        args: vec![],
    };
    let _ty1 = hir_ty(&mut hir, target);
    let inner = _expr1;
    let _expr2 = expr(
        &mut hir,
        Expr::Cast {
            expr: inner,
            ty: _ty1,
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);

    let e = _expr2;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, tcx.interner().mk_ty(Ty::Int(IntTy::I64)));
}

// ---------------------------------------------------------------------------
// Pattern checking
// ---------------------------------------------------------------------------

#[test]
fn pat_wild() {
    let mut hir = hir_crate();
    let _pat1 = pat(&mut hir, Pat::Wild);
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);
    let i32_ty = tcx.interner().mk_ty(Ty::Int(IntTy::I32));
    let p = _pat1;
    check_pat(&mut fcx, p, i32_ty);
    assert_eq!(fcx.results.pat_ty(_pat1), Some(i32_ty));
}

#[test]
fn pat_binding_inserts_local() {
    let mut hir = hir_crate();
    let _pat1 = pat(
        &mut hir,
        Pat::Binding {
            mode: BindingMode::ByValue,
            name: symbol(1),
            subpat: None,
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);
    let i32_ty = tcx.interner().mk_ty(Ty::Int(IntTy::I32));
    let p = _pat1;
    check_pat(&mut fcx, p, i32_ty);
    assert_eq!(fcx.results.local_ty(_pat1), Some(i32_ty));
    assert_eq!(fcx.results.pat_ty(_pat1), Some(i32_ty));
}

#[test]
fn pat_tuple() {
    let mut hir = hir_crate();
    let _pat2 = pat(&mut hir, Pat::Wild);
    let _pat3 = pat(&mut hir, Pat::Wild);
    let _pat1 = pat(
        &mut hir,
        Pat::Tuple {
            pats: vec![_pat2, _pat3],
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);
    let i32_ty = tcx.interner().mk_ty(Ty::Int(IntTy::I32));
    let p = _pat1;
    check_pat(&mut fcx, p, i32_ty);
    assert_eq!(fcx.results.pat_ty(_pat1), Some(i32_ty));
}

#[test]
fn pat_or() {
    let mut hir = hir_crate();
    let _pat2 = pat(&mut hir, Pat::Wild);
    let _pat3 = pat(&mut hir, Pat::Wild);
    let _pat1 = pat(
        &mut hir,
        Pat::Or {
            pats: vec![_pat2, _pat3],
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);
    let i32_ty = tcx.interner().mk_ty(Ty::Int(IntTy::I32));
    let p = _pat1;
    check_pat(&mut fcx, p, i32_ty);
    assert_eq!(fcx.results.pat_ty(_pat1), Some(i32_ty));
}

#[test]
fn pat_slice() {
    let mut hir = hir_crate();
    let _pat2 = pat(&mut hir, Pat::Wild);
    let _pat1 = pat(
        &mut hir,
        Pat::Slice {
            prefix: vec![_pat2],
            middle: None,
            suffix: vec![],
        },
    );
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);
    let i32_ty = tcx.interner().mk_ty(Ty::Int(IntTy::I32));
    let p = _pat1;
    check_pat(&mut fcx, p, i32_ty);
    assert_eq!(fcx.results.pat_ty(_pat1), Some(i32_ty));
}

#[test]
fn pat_err_is_error() {
    let mut hir = hir_crate();
    let _pat1 = pat(&mut hir, Pat::Err);
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);
    let p = _pat1;
    check_pat(&mut fcx, p, tcx.interner().mk_ty(Ty::Int(IntTy::I32)));
    assert_eq!(
        fcx.results.pat_ty(_pat1),
        Some(tcx.interner().mk_ty(Ty::Error))
    );
}

// ---------------------------------------------------------------------------
// Coercion checking
// ---------------------------------------------------------------------------

#[test]
fn coerce_exact_match() {
    let mut hir = hir_crate();
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);
    let i32_ty = tcx.interner().mk_ty(Ty::Int(IntTy::I32));
    let result = fcx.coerce(i32_ty, i32_ty);
    assert_eq!(result, Ok(i32_ty));
}

#[test]
fn coerce_mismatch_fails() {
    let mut hir = hir_crate();
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);
    let i32_ty = tcx.interner().mk_ty(Ty::Int(IntTy::I32));
    let bool_ty = tcx.interner().mk_ty(Ty::Bool);
    let result = fcx.coerce(i32_ty, bool_ty);
    assert!(result.is_err());
}

#[test]
fn coerce_anon_struct_width_subtyping() {
    let mut hir = hir_crate();
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);
    let i32_ty = tcx.interner().mk_ty(Ty::Int(IntTy::I32));
    let bool_ty = tcx.interner().mk_ty(Ty::Bool);

    let x_sym = Symbol::from(1);
    let y_sym = Symbol::from(2);

    let wide = tcx.interner().mk_ty(Ty::AnonStruct(AnonStructDef {
        fields: tcx.interner().mk_anon_struct_fields(&[
            AnonField {
                name: x_sym,
                ty: i32_ty,
            },
            AnonField {
                name: y_sym,
                ty: bool_ty,
            },
        ]),
    }));

    let narrow = tcx.interner().mk_ty(Ty::AnonStruct(AnonStructDef {
        fields: tcx.interner().mk_anon_struct_fields(&[AnonField {
            name: x_sym,
            ty: i32_ty,
        }]),
    }));

    assert_eq!(fcx.coerce(wide, narrow), Ok(narrow));
}

#[test]
fn coerce_anon_struct_width_subtyping_field_mismatch_fails() {
    let mut hir = hir_crate();
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);
    let i32_ty = tcx.interner().mk_ty(Ty::Int(IntTy::I32));
    let bool_ty = tcx.interner().mk_ty(Ty::Bool);

    let x_sym = Symbol::from(1);

    let wide = tcx.interner().mk_ty(Ty::AnonStruct(AnonStructDef {
        fields: tcx.interner().mk_anon_struct_fields(&[AnonField {
            name: x_sym,
            ty: i32_ty,
        }]),
    }));

    let narrow = tcx.interner().mk_ty(Ty::AnonStruct(AnonStructDef {
        fields: tcx.interner().mk_anon_struct_fields(&[AnonField {
            name: x_sym,
            ty: bool_ty,
        }]),
    }));

    assert!(fcx.coerce(wide, narrow).is_err());
}

// ---------------------------------------------------------------------------
// Writeback checking
// ---------------------------------------------------------------------------

#[test]
fn writeback_resolves_ty_var() {
    let mut hir = hir_crate();
    let _expr1 = expr(&mut hir, Expr::Err);
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);
    let ty_var = fcx.new_ty_var();
    let i32_ty = tcx.interner().mk_ty(Ty::Int(IntTy::I32));
    fcx.eq(ty_var, i32_ty).unwrap();
    fcx.results.expr_types.insert(_expr1, ty_var);

    writeback_types(&mut fcx);
    let resolved = fcx.results.expr_ty(_expr1).unwrap();
    assert_eq!(resolved, i32_ty);
}

#[test]
fn writeback_int_fallback_to_i32() {
    let mut hir = hir_crate();
    let _expr1 = expr(&mut hir, Expr::Err);
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);
    let int_var = fcx.new_int_var();
    fcx.results.expr_types.insert(_expr1, int_var);

    writeback_types(&mut fcx);
    let resolved = fcx.results.expr_ty(_expr1).unwrap();
    assert_eq!(resolved, tcx.interner().mk_ty(Ty::Int(IntTy::I32)));
}

#[test]
fn writeback_float_fallback_to_f64() {
    let mut hir = hir_crate();
    let _expr1 = expr(&mut hir, Expr::Err);
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);
    let float_var = fcx.new_float_var();
    fcx.results.expr_types.insert(_expr1, float_var);

    writeback_types(&mut fcx);
    let resolved = fcx.results.expr_ty(_expr1).unwrap();
    assert_eq!(resolved, tcx.interner().mk_ty(Ty::Float(FloatTy::F64)));
}

#[test]
fn writeback_unresolved_ty_var_becomes_error() {
    let mut hir = hir_crate();
    let _expr1 = expr(&mut hir, Expr::Err);
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);
    let ty_var = fcx.new_ty_var();
    fcx.results.expr_types.insert(_expr1, ty_var);

    writeback_types(&mut fcx);
    let resolved = fcx.results.expr_ty(_expr1).unwrap();
    assert_eq!(resolved, tcx.interner().mk_ty(Ty::Error));
}

// ---------------------------------------------------------------------------
// Body checking
// ---------------------------------------------------------------------------

#[test]
fn body_check_params_and_expr() {
    let mut hir = hir_crate();
    let _pat1 = pat(
        &mut hir,
        Pat::Binding {
            mode: BindingMode::ByValue,
            name: symbol(1),
            subpat: None,
        },
    );
    let _ty1 = hir_ty(
        &mut hir,
        hir::Ty::Path {
            res: Res::PrimTy {
                ty: yelang_hir::res::PrimTy::Int(yelang_hir::res::IntTy::I32),
            },
            args: vec![],
        },
    );
    let _expr2 = expr(
        &mut hir,
        Expr::Path {
            res: local_res(_pat1),
        },
    );
    let param = yelang_hir::hir::body::Param {
        pat: _pat1,
        ty: _ty1,
        span: dummy_span(),
    };
    let _body1 = body(&mut hir, vec![param], _expr2);
    let tcx = TyCtxt::new(hir);
    let i32_ty = tcx.interner().mk_ty(Ty::Int(IntTy::I32));
    let mut fcx = fcx_with_return_ty(&tcx, i32_ty);

    let body_id = _body1;
    check_body(&mut fcx, body_id);
    assert_eq!(fcx.results.local_ty(_pat1), Some(i32_ty));
}

// ---------------------------------------------------------------------------
// Error recovery
// ---------------------------------------------------------------------------

#[test]
fn expr_err_is_error() {
    let mut hir = hir_crate();
    let _expr1 = expr(&mut hir, Expr::Err);
    let tcx = TyCtxt::new(hir);
    let mut fcx = mk_fcx(&tcx);
    let e = _expr1;
    let ty = check_expr(&mut fcx, e);
    assert_eq!(ty, tcx.interner().mk_ty(Ty::Error));
}

// ---------------------------------------------------------------------------
// Solver integration tests
// ---------------------------------------------------------------------------

/// Build a HIR generic function `fn id<T>(x: T) -> T`.
fn build_generic_identity_fn(hir: &mut HirCrate, fn_def_id: DefId, param_def_id: DefId) -> BodyId {
    let param_ty = hir.alloc_ty(
        hir::Ty::Path {
            res: Res::Def {
                def_id: param_def_id,
            },
            args: vec![],
        },
        dummy_span(),
    );
    let sig = yelang_hir::hir::core::FnSig {
        inputs: vec![param_ty],
        output: param_ty,
        is_async: false,
        is_const: false,
        is_variadic: false,
        abi: None,
        bound_vars: vec![],
    };
    let param_pat = pat(hir, Pat::Wild);
    let param_expr = expr(
        hir,
        Expr::Path {
            res: local_res(PatId::default()),
        },
    );
    let body_id = body(
        hir,
        vec![Param {
            pat: param_pat,
            ty: param_ty,
            span: dummy_span(),
        }],
        param_expr,
    );

    hir.items.insert(
        fn_def_id,
        Some(yelang_hir::hir::item::Item {
            def_id: fn_def_id,
            ident: yelang_ast::Ident::new(symbol(2), dummy_span()),
            kind: yelang_hir::hir::item::ItemKind::Fn {
                sig,
                body: body_id,
                generics: yelang_hir::hir::core::Generics {
                    params: vec![yelang_hir::hir::core::GenericParam::Type {
                        def_id: param_def_id,
                        name: yelang_ast::Ident::new(symbol(1), dummy_span()),
                        bounds: vec![],
                        default: None,
                        span: dummy_span(),
                    }],
                    where_clause: None,
                    span: dummy_span(),
                },
            },
            vis: yelang_hir::hir::core::Visibility::Public(dummy_span()),
            attrs: vec![],
            span: dummy_span(),
        }),
    );
    body_id
}

#[test]
fn generic_fn_call_instantiates_params() {
    let mut hir = hir_crate();
    let fn_def_id = def_id(2);
    let param_def_id = def_id(3);
    build_generic_identity_fn(&mut hir, fn_def_id, param_def_id);

    let mut tcx = TyCtxt::new(hir);
    collect_crate_types(&mut tcx);

    // Build body: `id(42)`.
    let lit_expr = expr(
        &mut tcx.crate_hir_mut(),
        Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(42),
                suffix: None,
            }),
        },
    );
    let path_expr = expr(&mut tcx.crate_hir_mut(), Expr::Path { res: def_res(2) });
    let call_expr = expr(
        &mut tcx.crate_hir_mut(),
        Expr::Call {
            func: path_expr,
            args: vec![lit_expr],
        },
    );
    let body_id = body(&mut tcx.crate_hir_mut(), vec![], call_expr);

    let i32_ty = tcx.interner().mk_ty(Ty::Int(IntTy::I32));
    let mut fcx = FnCtxt::new(&tcx, def_id(4), i32_ty);
    check_body(&mut fcx, body_id);

    // The return type of `id(42)` should be i32 after unification.
    let call_ty = fcx.results.expr_types.get(&call_expr).copied().unwrap();
    assert_eq!(call_ty, i32_ty);
}

#[test]
fn builtin_trait_obligation_is_proven() {
    let mut hir = hir_crate();
    let fn_def_id = def_id(2);
    let param_def_id = def_id(3);
    let trait_def_id = def_id(4);

    // Generic param with a `Clone` bound.
    let param_ty = hir.alloc_ty(
        hir::Ty::Path {
            res: Res::Def {
                def_id: param_def_id,
            },
            args: vec![],
        },
        dummy_span(),
    );
    let clone_bound = yelang_hir::hir::core::TraitBound {
        path: Res::Def {
            def_id: trait_def_id,
        },
        args: vec![],
        span: dummy_span(),
    };
    let sig = yelang_hir::hir::core::FnSig {
        inputs: vec![param_ty],
        output: param_ty,
        is_async: false,
        is_const: false,
        is_variadic: false,
        abi: None,
        bound_vars: vec![],
    };
    let param_pat = pat(&mut hir, Pat::Wild);
    let param_expr = expr(
        &mut hir,
        Expr::Path {
            res: local_res(PatId::default()),
        },
    );
    let body_id = body(
        &mut hir,
        vec![Param {
            pat: param_pat,
            ty: param_ty,
            span: dummy_span(),
        }],
        param_expr,
    );
    hir.items.insert(
        fn_def_id,
        Some(yelang_hir::hir::item::Item {
            def_id: fn_def_id,
            ident: yelang_ast::Ident::new(symbol(2), dummy_span()),
            kind: yelang_hir::hir::item::ItemKind::Fn {
                sig,
                body: body_id,
                generics: yelang_hir::hir::core::Generics {
                    params: vec![yelang_hir::hir::core::GenericParam::Type {
                        def_id: param_def_id,
                        name: yelang_ast::Ident::new(symbol(1), dummy_span()),
                        bounds: vec![clone_bound],
                        default: None,
                        span: dummy_span(),
                    }],
                    where_clause: None,
                    span: dummy_span(),
                },
            },
            vis: yelang_hir::hir::core::Visibility::Public(dummy_span()),
            attrs: vec![],
            span: dummy_span(),
        }),
    );

    // Trait definition for Clone.
    hir.items.insert(
        trait_def_id,
        Some(yelang_hir::hir::item::Item {
            def_id: trait_def_id,
            ident: yelang_ast::Ident::new(symbol(5), dummy_span()),
            kind: yelang_hir::hir::item::ItemKind::Trait {
                items: vec![],
                generics: yelang_hir::hir::core::Generics {
                    params: vec![],
                    where_clause: None,
                    span: dummy_span(),
                },
                super_traits: vec![],
            },
            vis: yelang_hir::hir::core::Visibility::Public(dummy_span()),
            attrs: vec![],
            span: dummy_span(),
        }),
    );
    hir.traits.insert(
        trait_def_id,
        Some(yelang_hir::hir::core::Trait {
            name: yelang_ast::Ident::new(symbol(5), dummy_span()),
            generics: yelang_hir::hir::core::Generics {
                params: vec![],
                where_clause: None,
                span: dummy_span(),
            },
            super_traits: vec![],
            items: vec![],
            span: dummy_span(),
        }),
    );

    let mut tcx = TyCtxt::new(hir);
    collect_crate_types(&mut tcx);
    tcx.register_builtin_trait(trait_def_id, BuiltinTraitKind::Clone);
    tcx.populate_solver_caches();

    // Body: `clone_fn(42)`.
    let lit_expr = expr(
        &mut tcx.crate_hir_mut(),
        Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(42),
                suffix: None,
            }),
        },
    );
    let path_expr = expr(&mut tcx.crate_hir_mut(), Expr::Path { res: def_res(2) });
    let call_expr = expr(
        &mut tcx.crate_hir_mut(),
        Expr::Call {
            func: path_expr,
            args: vec![lit_expr],
        },
    );
    let body_id = body(&mut tcx.crate_hir_mut(), vec![], call_expr);

    let i32_ty = tcx.interner().mk_ty(Ty::Int(IntTy::I32));
    let mut fcx = FnCtxt::new(&tcx, def_id(6), i32_ty);
    check_body(&mut fcx, body_id);

    // The `i32: Clone` obligation should be proven by the built-in rule.
    let unproven = fcx.prove_obligations();
    assert!(
        unproven.is_empty(),
        "expected all obligations proven, got {:?}",
        unproven
    );
}

#[test]
fn where_clause_with_generic_args_is_proven_via_param_env() {
    let mut hir = hir_crate();

    let bar_trait = def_id(10);
    let needs_bar_fn = def_id(20);
    let foo_fn = def_id(30);
    let t_param = def_id(40);
    let u_param = def_id(41);
    let v_param = def_id(50);
    let w_param = def_id(51);

    // `trait Bar<T>`
    hir.items.insert(
        bar_trait,
        Some(yelang_hir::hir::item::Item {
            def_id: bar_trait,
            ident: yelang_ast::Ident::new(symbol(1), dummy_span()),
            kind: yelang_hir::hir::item::ItemKind::Trait {
                items: vec![],
                generics: yelang_hir::hir::core::Generics {
                    params: vec![yelang_hir::hir::core::GenericParam::Type {
                        def_id: t_param,
                        name: yelang_ast::Ident::new(symbol(1), dummy_span()),
                        bounds: vec![],
                        default: None,
                        span: dummy_span(),
                    }],
                    where_clause: None,
                    span: dummy_span(),
                },
                super_traits: vec![],
            },
            vis: yelang_hir::hir::core::Visibility::Public(dummy_span()),
            attrs: vec![],
            span: dummy_span(),
        }),
    );
    hir.traits.insert(
        bar_trait,
        Some(yelang_hir::hir::core::Trait {
            name: yelang_ast::Ident::new(symbol(1), dummy_span()),
            generics: yelang_hir::hir::core::Generics {
                params: vec![yelang_hir::hir::core::GenericParam::Type {
                    def_id: t_param,
                    name: yelang_ast::Ident::new(symbol(1), dummy_span()),
                    bounds: vec![],
                    default: None,
                    span: dummy_span(),
                }],
                where_clause: None,
                span: dummy_span(),
            },
            super_traits: vec![],
            items: vec![],
            span: dummy_span(),
        }),
    );

    let t_ty = hir.alloc_ty(
        hir::Ty::Path {
            res: Res::Def { def_id: t_param },
            args: vec![],
        },
        dummy_span(),
    );
    let u_ty = hir.alloc_ty(
        hir::Ty::Path {
            res: Res::Def { def_id: u_param },
            args: vec![],
        },
        dummy_span(),
    );
    let v_ty = hir.alloc_ty(
        hir::Ty::Path {
            res: Res::Def { def_id: v_param },
            args: vec![],
        },
        dummy_span(),
    );
    let w_ty = hir.alloc_ty(
        hir::Ty::Path {
            res: Res::Def { def_id: w_param },
            args: vec![],
        },
        dummy_span(),
    );

    let bar_bound_for_u = yelang_hir::hir::core::TraitBound {
        path: Res::Def { def_id: bar_trait },
        args: vec![yelang_hir::hir::ty::GenericArg::Type(u_ty)],
        span: dummy_span(),
    };
    let bar_bound_for_w = yelang_hir::hir::core::TraitBound {
        path: Res::Def { def_id: bar_trait },
        args: vec![yelang_hir::hir::ty::GenericArg::Type(w_ty)],
        span: dummy_span(),
    };

    // `fn needs_bar<V, W>(v: V) -> W where V: Bar<W>`
    let needs_bar_sig = yelang_hir::hir::core::FnSig {
        inputs: vec![v_ty],
        output: w_ty,
        is_async: false,
        is_const: false,
        is_variadic: false,
        abi: None,
        bound_vars: vec![],
    };
    let needs_bar_value = expr(&mut hir, Expr::Err);
    let needs_bar_body = body(&mut hir, vec![], needs_bar_value);
    hir.items.insert(
        needs_bar_fn,
        Some(yelang_hir::hir::item::Item {
            def_id: needs_bar_fn,
            ident: yelang_ast::Ident::new(symbol(2), dummy_span()),
            kind: yelang_hir::hir::item::ItemKind::Fn {
                sig: needs_bar_sig,
                body: needs_bar_body,
                generics: yelang_hir::hir::core::Generics {
                    params: vec![
                        yelang_hir::hir::core::GenericParam::Type {
                            def_id: v_param,
                            name: yelang_ast::Ident::new(symbol(3), dummy_span()),
                            bounds: vec![],
                            default: None,
                            span: dummy_span(),
                        },
                        yelang_hir::hir::core::GenericParam::Type {
                            def_id: w_param,
                            name: yelang_ast::Ident::new(symbol(4), dummy_span()),
                            bounds: vec![],
                            default: None,
                            span: dummy_span(),
                        },
                    ],
                    where_clause: Some(yelang_hir::hir::core::WhereClause {
                        predicates: vec![yelang_hir::hir::core::WherePredicate::TraitBound {
                            ty: v_ty,
                            bounds: vec![bar_bound_for_w],
                        }],
                        span: dummy_span(),
                    }),
                    span: dummy_span(),
                },
            },
            vis: yelang_hir::hir::core::Visibility::Public(dummy_span()),
            attrs: vec![],
            span: dummy_span(),
        }),
    );

    // `fn foo<T, U>(x: T) -> U where T: Bar<U> { needs_bar(x) }`
    let x_pat = pat(
        &mut hir,
        Pat::Binding {
            mode: BindingMode::ByValue,
            name: symbol(5),
            subpat: None,
        },
    );
    let x_expr = expr(
        &mut hir,
        Expr::Path {
            res: local_res(x_pat),
        },
    );
    let needs_bar_path = expr(&mut hir, Expr::Path { res: def_res(20) });
    let call_expr = expr(
        &mut hir,
        Expr::Call {
            func: needs_bar_path,
            args: vec![x_expr],
        },
    );
    let foo_sig = yelang_hir::hir::core::FnSig {
        inputs: vec![t_ty],
        output: u_ty,
        is_async: false,
        is_const: false,
        is_variadic: false,
        abi: None,
        bound_vars: vec![],
    };
    let foo_body = body(
        &mut hir,
        vec![yelang_hir::hir::body::Param {
            pat: x_pat,
            ty: t_ty,
            span: dummy_span(),
        }],
        call_expr,
    );
    hir.items.insert(
        foo_fn,
        Some(yelang_hir::hir::item::Item {
            def_id: foo_fn,
            ident: yelang_ast::Ident::new(symbol(6), dummy_span()),
            kind: yelang_hir::hir::item::ItemKind::Fn {
                sig: foo_sig,
                body: foo_body,
                generics: yelang_hir::hir::core::Generics {
                    params: vec![
                        yelang_hir::hir::core::GenericParam::Type {
                            def_id: t_param,
                            name: yelang_ast::Ident::new(symbol(1), dummy_span()),
                            bounds: vec![],
                            default: None,
                            span: dummy_span(),
                        },
                        yelang_hir::hir::core::GenericParam::Type {
                            def_id: u_param,
                            name: yelang_ast::Ident::new(symbol(2), dummy_span()),
                            bounds: vec![],
                            default: None,
                            span: dummy_span(),
                        },
                    ],
                    where_clause: Some(yelang_hir::hir::core::WhereClause {
                        predicates: vec![yelang_hir::hir::core::WherePredicate::TraitBound {
                            ty: t_ty,
                            bounds: vec![bar_bound_for_u],
                        }],
                        span: dummy_span(),
                    }),
                    span: dummy_span(),
                },
            },
            vis: yelang_hir::hir::core::Visibility::Public(dummy_span()),
            attrs: vec![],
            span: dummy_span(),
        }),
    );

    let mut tcx = TyCtxt::new(hir);
    collect_crate_types(&mut tcx);
    tcx.populate_solver_caches();

    let interner = tcx.interner();
    let return_ty = interner.mk_ty(Ty::Param(yelang_ty::ty::ParamTy {
        index: 1,
        name: symbol(2),
    }));
    let mut fcx = FnCtxt::new(&tcx, foo_fn, return_ty);
    check_body(&mut fcx, foo_body);

    let unproven = fcx.prove_obligations();
    assert!(
        unproven.is_empty(),
        "expected where-clause obligation with generic args to be proven via param-env, got {:?}",
        unproven
    );
}

#[test]
fn inherent_method_call_with_autoref() {
    let mut hir = hir_crate();

    let struct_def_id = def_id(2);
    let impl_def_id = def_id(3);
    let method_def_id = def_id(4);

    let foo_ty_id = hir.alloc_ty(
        hir::Ty::Path {
            res: Res::Def {
                def_id: struct_def_id,
            },
            args: vec![],
        },
        dummy_span(),
    );

    // `struct Foo;`
    hir.items.insert(
        struct_def_id,
        Some(yelang_hir::hir::item::Item {
            def_id: struct_def_id,
            ident: yelang_ast::Ident::new(symbol(1), dummy_span()),
            kind: yelang_hir::hir::item::ItemKind::Struct {
                data: yelang_hir::hir::adt::VariantData::Unit,
                generics: yelang_hir::hir::core::Generics {
                    params: vec![],
                    where_clause: None,
                    span: dummy_span(),
                },
            },
            vis: yelang_hir::hir::core::Visibility::Public(dummy_span()),
            attrs: vec![],
            span: dummy_span(),
        }),
    );

    // `impl Foo { fn bar(&self, x: i32) -> bool { ... } }`
    let i32_hir_ty = hir_i32_id(&mut hir);
    let bool_hir_ty = hir.alloc_ty(
        hir::Ty::Path {
            res: Res::PrimTy {
                ty: yelang_hir::res::PrimTy::Bool,
            },
            args: vec![],
        },
        dummy_span(),
    );
    let ref_foo_ty = hir.alloc_ty(
        hir::Ty::Ref {
            mutability: yelang_ast::Mutability::Immutable,
            ty: foo_ty_id,
        },
        dummy_span(),
    );

    let bar_sig = yelang_hir::hir::core::FnSig {
        inputs: vec![ref_foo_ty, i32_hir_ty],
        output: bool_hir_ty,
        is_async: false,
        is_const: false,
        is_variadic: false,
        abi: None,
        bound_vars: vec![],
    };
    let bar_body_value = expr(&mut hir, Expr::Err);
    let bar_body = body(&mut hir, vec![], bar_body_value);

    hir.impls.push(yelang_hir::hir::core::Impl {
        def_id: impl_def_id,
        generics: yelang_hir::hir::core::Generics {
            params: vec![],
            where_clause: None,
            span: dummy_span(),
        },
        self_ty: foo_ty_id,
        of_trait: None,
        items: vec![yelang_hir::hir::core::ImplItem {
            def_id: method_def_id,
            ident: yelang_ast::Ident::new(symbol(10), dummy_span()),
            kind: yelang_hir::hir::core::ImplItemKind::Fn {
                sig: bar_sig,
                body: bar_body,
            },
            attrs: vec![],
            span: dummy_span(),
            defaultness: yelang_hir::hir::core::Defaultness::Final,
        }],
        polarity: yelang_hir::hir::core::ImplPolarity::Positive,
        span: dummy_span(),
    });

    let mut tcx = TyCtxt::new(hir);
    collect_crate_types(&mut tcx);

    let foo_pat = pat(&mut tcx.crate_hir_mut(), Pat::Wild);
    let receiver = expr(
        &mut tcx.crate_hir_mut(),
        Expr::Path {
            res: local_res(foo_pat),
        },
    );
    let arg = expr(
        &mut tcx.crate_hir_mut(),
        Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(42),
                suffix: None,
            }),
        },
    );
    let call = expr(
        &mut tcx.crate_hir_mut(),
        Expr::MethodCall {
            receiver,
            method: yelang_ast::Ident::new(symbol(10), dummy_span()),
            args: vec![arg],
            trait_def_id: None,
        },
    );

    let mut fcx = mk_fcx(&tcx);
    let foo_ty = fcx
        .tcx
        .item_ty(struct_def_id)
        .expect("Foo should have a type");
    fcx.insert_local(foo_pat, foo_ty);

    let call_ty = check_expr(&mut fcx, call);
    let bool_ty = tcx.interner().mk_ty(Ty::Bool);
    assert_eq!(call_ty, bool_ty);
}

#[test]
fn trait_method_call_extension() {
    let mut hir = hir_crate();

    let struct_def_id = def_id(2);
    let trait_def_id = def_id(3);
    let trait_method_def_id = def_id(4);
    let impl_def_id = def_id(5);
    let impl_method_def_id = def_id(6);

    let foo_ty_id = hir.alloc_ty(
        hir::Ty::Path {
            res: Res::Def {
                def_id: struct_def_id,
            },
            args: vec![],
        },
        dummy_span(),
    );

    // `struct Foo;`
    hir.items.insert(
        struct_def_id,
        Some(yelang_hir::hir::item::Item {
            def_id: struct_def_id,
            ident: yelang_ast::Ident::new(symbol(1), dummy_span()),
            kind: yelang_hir::hir::item::ItemKind::Struct {
                data: yelang_hir::hir::adt::VariantData::Unit,
                generics: yelang_hir::hir::core::Generics {
                    params: vec![],
                    where_clause: None,
                    span: dummy_span(),
                },
            },
            vis: yelang_hir::hir::core::Visibility::Public(dummy_span()),
            attrs: vec![],
            span: dummy_span(),
        }),
    );

    // `trait Greet { fn greet(&self) -> bool; }`
    let bool_hir_ty = hir.alloc_ty(
        hir::Ty::Path {
            res: Res::PrimTy {
                ty: yelang_hir::res::PrimTy::Bool,
            },
            args: vec![],
        },
        dummy_span(),
    );
    let ref_foo_ty = hir.alloc_ty(
        hir::Ty::Ref {
            mutability: yelang_ast::Mutability::Immutable,
            ty: foo_ty_id,
        },
        dummy_span(),
    );
    let greet_sig = yelang_hir::hir::core::FnSig {
        inputs: vec![ref_foo_ty],
        output: bool_hir_ty,
        is_async: false,
        is_const: false,
        is_variadic: false,
        abi: None,
        bound_vars: vec![],
    };
    let greet_trait_item = yelang_hir::hir::core::TraitItem {
        def_id: trait_method_def_id,
        ident: yelang_ast::Ident::new(symbol(10), dummy_span()),
        kind: yelang_hir::hir::core::TraitItemKind::Fn {
            sig: greet_sig.clone(),
            default: None,
        },
        attrs: vec![],
        span: dummy_span(),
    };
    hir.traits.insert(
        trait_def_id,
        Some(yelang_hir::hir::core::Trait {
            name: yelang_ast::Ident::new(symbol(2), dummy_span()),
            generics: yelang_hir::hir::core::Generics {
                params: vec![],
                where_clause: None,
                span: dummy_span(),
            },
            super_traits: vec![],
            items: vec![greet_trait_item],
            span: dummy_span(),
        }),
    );

    // `impl Greet for Foo { fn greet(&self) -> bool { ... } }`
    let impl_body_value = expr(&mut hir, Expr::Err);
    let impl_body = body(&mut hir, vec![], impl_body_value);
    hir.impls.push(yelang_hir::hir::core::Impl {
        def_id: impl_def_id,
        generics: yelang_hir::hir::core::Generics {
            params: vec![],
            where_clause: None,
            span: dummy_span(),
        },
        self_ty: foo_ty_id,
        of_trait: Some(yelang_hir::hir::core::TraitRef {
            path: Res::Def {
                def_id: trait_def_id,
            },
            span: dummy_span(),
        }),
        items: vec![yelang_hir::hir::core::ImplItem {
            def_id: impl_method_def_id,
            ident: yelang_ast::Ident::new(symbol(10), dummy_span()),
            kind: yelang_hir::hir::core::ImplItemKind::Fn {
                sig: greet_sig.clone(),
                body: impl_body,
            },
            attrs: vec![],
            span: dummy_span(),
            defaultness: yelang_hir::hir::core::Defaultness::Final,
        }],
        polarity: yelang_hir::hir::core::ImplPolarity::Positive,
        span: dummy_span(),
    });

    let mut tcx = TyCtxt::new(hir);
    collect_crate_types(&mut tcx);

    let foo_pat = pat(&mut tcx.crate_hir_mut(), Pat::Wild);
    let receiver = expr(
        &mut tcx.crate_hir_mut(),
        Expr::Path {
            res: local_res(foo_pat),
        },
    );
    let call = expr(
        &mut tcx.crate_hir_mut(),
        Expr::MethodCall {
            receiver,
            method: yelang_ast::Ident::new(symbol(10), dummy_span()),
            args: vec![],
            trait_def_id: None,
        },
    );

    let mut fcx = mk_fcx(&tcx);
    let foo_ty = fcx
        .tcx
        .item_ty(struct_def_id)
        .expect("Foo should have a type");
    fcx.insert_local(foo_pat, foo_ty);

    let call_ty = check_expr(&mut fcx, call);
    let bool_ty = tcx.interner().mk_ty(Ty::Bool);
    assert_eq!(call_ty, bool_ty);

    let unproven = fcx.prove_obligations();
    assert!(
        unproven.is_empty(),
        "expected trait method obligation to be proven, got {:?}",
        unproven
    );
}

#[test]
fn identity_args_uses_correct_param_indices() {
    let mut hir = hir_crate();
    let def_id_t = def_id(3);
    let def_id_u = def_id(4);
    let struct_def_id = def_id(2);

    let t_ty = hir.alloc_ty(
        hir::Ty::Path {
            res: Res::Def { def_id: def_id_t },
            args: vec![],
        },
        dummy_span(),
    );
    let _u_ty = hir.alloc_ty(
        hir::Ty::Path {
            res: Res::Def { def_id: def_id_u },
            args: vec![],
        },
        dummy_span(),
    );
    let field = yelang_hir::hir::adt::FieldDef {
        def_id: def_id(10),
        ident: yelang_ast::Ident::new(symbol(1), dummy_span()),
        ty: t_ty,
        span: dummy_span(),
        vis: yelang_hir::hir::core::Visibility::Public(dummy_span()),
        attrs: vec![],
    };
    hir.items.insert(
        struct_def_id,
        Some(yelang_hir::hir::item::Item {
            def_id: struct_def_id,
            ident: yelang_ast::Ident::new(symbol(3), dummy_span()),
            kind: yelang_hir::hir::item::ItemKind::Struct {
                data: yelang_hir::hir::adt::VariantData::Struct {
                    fields: vec![field],
                },
                generics: yelang_hir::hir::core::Generics {
                    params: vec![
                        yelang_hir::hir::core::GenericParam::Type {
                            def_id: def_id_t,
                            name: yelang_ast::Ident::new(symbol(1), dummy_span()),
                            bounds: vec![],
                            default: None,
                            span: dummy_span(),
                        },
                        yelang_hir::hir::core::GenericParam::Type {
                            def_id: def_id_u,
                            name: yelang_ast::Ident::new(symbol(2), dummy_span()),
                            bounds: vec![],
                            default: None,
                            span: dummy_span(),
                        },
                    ],
                    where_clause: None,
                    span: dummy_span(),
                },
            },
            vis: yelang_hir::hir::core::Visibility::Public(dummy_span()),
            attrs: vec![],
            span: dummy_span(),
        }),
    );

    let mut tcx = TyCtxt::new(hir);
    collect_crate_types(&mut tcx);

    let struct_ty = tcx.item_ty(struct_def_id).unwrap();
    match tcx.interner().ty(struct_ty) {
        Ty::Adt(_, args) => {
            assert_eq!(args.len(), 2);
            match tcx.interner().ty(args[0].expect_type()) {
                Ty::Param(p) => assert_eq!(p.index, 0),
                _ => panic!("expected param 0 for T"),
            }
            match tcx.interner().ty(args[1].expect_type()) {
                Ty::Param(p) => assert_eq!(p.index, 1),
                _ => panic!("expected param 1 for U"),
            }
        }
        _ => panic!("expected adt"),
    }
}

#[test]
fn method_dispatch_via_deref_trait() {
    let mut hir = hir_crate();

    let inner_struct = def_id(2);
    let wrapper_struct = def_id(3);
    let deref_trait = def_id(4);
    let target_assoc = def_id(5);
    let deref_impl = def_id(6);
    let target_impl_item = def_id(7);
    let inner_method = def_id(8);
    let inner_impl = def_id(9);

    let inner_ty_id = hir.alloc_ty(
        hir::Ty::Path {
            res: Res::Def {
                def_id: inner_struct,
            },
            args: vec![],
        },
        dummy_span(),
    );
    let wrapper_ty_id = hir.alloc_ty(
        hir::Ty::Path {
            res: Res::Def {
                def_id: wrapper_struct,
            },
            args: vec![],
        },
        dummy_span(),
    );

    // `struct Inner;`
    hir.items.insert(
        inner_struct,
        Some(yelang_hir::hir::item::Item {
            def_id: inner_struct,
            ident: yelang_ast::Ident::new(symbol(1), dummy_span()),
            kind: yelang_hir::hir::item::ItemKind::Struct {
                data: yelang_hir::hir::adt::VariantData::Unit,
                generics: yelang_hir::hir::core::Generics {
                    params: vec![],
                    where_clause: None,
                    span: dummy_span(),
                },
            },
            vis: yelang_hir::hir::core::Visibility::Public(dummy_span()),
            attrs: vec![],
            span: dummy_span(),
        }),
    );

    // `struct Wrapper;`
    hir.items.insert(
        wrapper_struct,
        Some(yelang_hir::hir::item::Item {
            def_id: wrapper_struct,
            ident: yelang_ast::Ident::new(symbol(2), dummy_span()),
            kind: yelang_hir::hir::item::ItemKind::Struct {
                data: yelang_hir::hir::adt::VariantData::Unit,
                generics: yelang_hir::hir::core::Generics {
                    params: vec![],
                    where_clause: None,
                    span: dummy_span(),
                },
            },
            vis: yelang_hir::hir::core::Visibility::Public(dummy_span()),
            attrs: vec![],
            span: dummy_span(),
        }),
    );

    // `trait Deref { type Target; }`
    let target_trait_item = yelang_hir::hir::core::TraitItem {
        def_id: target_assoc,
        ident: yelang_ast::Ident::new(symbol(21), dummy_span()),
        kind: yelang_hir::hir::core::TraitItemKind::Type {
            bounds: vec![],
            default: None,
        },
        attrs: vec![],
        span: dummy_span(),
    };
    hir.traits.insert(
        deref_trait,
        Some(yelang_hir::hir::core::Trait {
            name: yelang_ast::Ident::new(symbol(20), dummy_span()),
            generics: yelang_hir::hir::core::Generics {
                params: vec![],
                where_clause: None,
                span: dummy_span(),
            },
            super_traits: vec![],
            items: vec![target_trait_item],
            span: dummy_span(),
        }),
    );

    // `impl Deref for Wrapper { type Target = Inner; }`
    let target_impl_assoc = yelang_hir::hir::core::ImplItem {
        def_id: target_impl_item,
        ident: yelang_ast::Ident::new(symbol(21), dummy_span()),
        kind: yelang_hir::hir::core::ImplItemKind::Type { ty: inner_ty_id },
        attrs: vec![],
        span: dummy_span(),
        defaultness: yelang_hir::hir::core::Defaultness::Final,
    };
    hir.impls.push(yelang_hir::hir::core::Impl {
        def_id: deref_impl,
        generics: yelang_hir::hir::core::Generics {
            params: vec![],
            where_clause: None,
            span: dummy_span(),
        },
        self_ty: wrapper_ty_id,
        of_trait: Some(yelang_hir::hir::core::TraitRef {
            path: Res::Def {
                def_id: deref_trait,
            },
            span: dummy_span(),
        }),
        items: vec![target_impl_assoc],
        polarity: yelang_hir::hir::core::ImplPolarity::Positive,
        span: dummy_span(),
    });

    // `impl Inner { fn get(&self) -> bool { ... } }`
    let bool_hir_ty = hir.alloc_ty(
        hir::Ty::Path {
            res: Res::PrimTy {
                ty: yelang_hir::res::PrimTy::Bool,
            },
            args: vec![],
        },
        dummy_span(),
    );
    let ref_inner_ty = hir.alloc_ty(
        hir::Ty::Ref {
            mutability: yelang_ast::Mutability::Immutable,
            ty: inner_ty_id,
        },
        dummy_span(),
    );
    let get_sig = yelang_hir::hir::core::FnSig {
        inputs: vec![ref_inner_ty],
        output: bool_hir_ty,
        is_async: false,
        is_const: false,
        is_variadic: false,
        abi: None,
        bound_vars: vec![],
    };
    let get_body_value = expr(&mut hir, Expr::Err);
    let get_body = body(&mut hir, vec![], get_body_value);
    let get_impl_item = yelang_hir::hir::core::ImplItem {
        def_id: inner_method,
        ident: yelang_ast::Ident::new(symbol(30), dummy_span()),
        kind: yelang_hir::hir::core::ImplItemKind::Fn {
            sig: get_sig,
            body: get_body,
        },
        attrs: vec![],
        span: dummy_span(),
        defaultness: yelang_hir::hir::core::Defaultness::Final,
    };
    hir.impls.push(yelang_hir::hir::core::Impl {
        def_id: inner_impl,
        generics: yelang_hir::hir::core::Generics {
            params: vec![],
            where_clause: None,
            span: dummy_span(),
        },
        self_ty: inner_ty_id,
        of_trait: None,
        items: vec![get_impl_item],
        polarity: yelang_hir::hir::core::ImplPolarity::Positive,
        span: dummy_span(),
    });

    let mut tcx = TyCtxt::new(hir);
    collect_crate_types(&mut tcx);
    tcx.register_deref_lang_item(deref_trait, target_assoc);
    tcx.populate_solver_caches();

    let wrapper_pat = pat(&mut tcx.crate_hir_mut(), Pat::Wild);
    let receiver = expr(
        &mut tcx.crate_hir_mut(),
        Expr::Path {
            res: local_res(wrapper_pat),
        },
    );
    let call = expr(
        &mut tcx.crate_hir_mut(),
        Expr::MethodCall {
            receiver,
            method: yelang_ast::Ident::new(symbol(30), dummy_span()),
            args: vec![],
            trait_def_id: None,
        },
    );

    let mut fcx = mk_fcx(&tcx);
    let wrapper_ty = fcx
        .tcx
        .item_ty(wrapper_struct)
        .expect("Wrapper should have a type");
    let inner_ty = fcx
        .tcx
        .item_ty(inner_struct)
        .expect("Inner should have a type");
    fcx.insert_local(wrapper_pat, wrapper_ty);

    let call_ty = check_expr(&mut fcx, call);
    let bool_ty = tcx.interner().mk_ty(Ty::Bool);
    assert_eq!(call_ty, bool_ty);

    let unproven = fcx.prove_obligations();
    assert!(
        unproven.is_empty(),
        "expected Deref obligations to be proven, got {:?}",
        unproven
    );

    let adjustments = fcx.results.expr_adjustments(receiver);
    assert!(
        adjustments.iter().any(|a| matches!(
            a,
            Adjustment::DerefTrait { target, .. } if *target == inner_ty
        )),
        "expected a DerefTrait adjustment to Inner, got {:?}",
        adjustments
    );
}

#[test]
fn deref_chain_two_steps() {
    let mut hir = hir_crate();

    let core_struct = def_id(10);
    let inner_struct = def_id(11);
    let wrapper_struct = def_id(12);
    let deref_trait = def_id(13);
    let target_assoc = def_id(14);
    let inner_deref_impl = def_id(15);
    let inner_target_impl = def_id(16);
    let wrapper_deref_impl = def_id(17);
    let wrapper_target_impl = def_id(18);
    let core_method = def_id(19);
    let core_impl = def_id(20);

    let core_ty_id = hir.alloc_ty(
        hir::Ty::Path {
            res: Res::Def {
                def_id: core_struct,
            },
            args: vec![],
        },
        dummy_span(),
    );
    let inner_ty_id = hir.alloc_ty(
        hir::Ty::Path {
            res: Res::Def {
                def_id: inner_struct,
            },
            args: vec![],
        },
        dummy_span(),
    );
    let wrapper_ty_id = hir.alloc_ty(
        hir::Ty::Path {
            res: Res::Def {
                def_id: wrapper_struct,
            },
            args: vec![],
        },
        dummy_span(),
    );

    // `struct Core; struct Inner; struct Wrapper;`
    for (def_id_, symbol_) in [
        (core_struct, symbol(10)),
        (inner_struct, symbol(11)),
        (wrapper_struct, symbol(12)),
    ] {
        hir.items.insert(
            def_id_,
            Some(yelang_hir::hir::item::Item {
                def_id: def_id_,
                ident: yelang_ast::Ident::new(symbol_, dummy_span()),
                kind: yelang_hir::hir::item::ItemKind::Struct {
                    data: yelang_hir::hir::adt::VariantData::Unit,
                    generics: yelang_hir::hir::core::Generics {
                        params: vec![],
                        where_clause: None,
                        span: dummy_span(),
                    },
                },
                vis: yelang_hir::hir::core::Visibility::Public(dummy_span()),
                attrs: vec![],
                span: dummy_span(),
            }),
        );
    }

    // `trait Deref { type Target; }`
    let target_trait_item = yelang_hir::hir::core::TraitItem {
        def_id: target_assoc,
        ident: yelang_ast::Ident::new(symbol(21), dummy_span()),
        kind: yelang_hir::hir::core::TraitItemKind::Type {
            bounds: vec![],
            default: None,
        },
        attrs: vec![],
        span: dummy_span(),
    };
    hir.traits.insert(
        deref_trait,
        Some(yelang_hir::hir::core::Trait {
            name: yelang_ast::Ident::new(symbol(20), dummy_span()),
            generics: yelang_hir::hir::core::Generics {
                params: vec![],
                where_clause: None,
                span: dummy_span(),
            },
            super_traits: vec![],
            items: vec![target_trait_item],
            span: dummy_span(),
        }),
    );

    // `impl Deref for Wrapper { type Target = Inner; }`
    // `impl Deref for Inner { type Target = Core; }`
    for (self_ty, target_ty, impl_def_id, type_item_def_id) in [
        (
            wrapper_ty_id,
            inner_ty_id,
            wrapper_deref_impl,
            wrapper_target_impl,
        ),
        (inner_ty_id, core_ty_id, inner_deref_impl, inner_target_impl),
    ] {
        let type_item = yelang_hir::hir::core::ImplItem {
            def_id: type_item_def_id,
            ident: yelang_ast::Ident::new(symbol(21), dummy_span()),
            kind: yelang_hir::hir::core::ImplItemKind::Type { ty: target_ty },
            attrs: vec![],
            span: dummy_span(),
            defaultness: yelang_hir::hir::core::Defaultness::Final,
        };
        hir.impls.push(yelang_hir::hir::core::Impl {
            def_id: impl_def_id,
            generics: yelang_hir::hir::core::Generics {
                params: vec![],
                where_clause: None,
                span: dummy_span(),
            },
            self_ty,
            of_trait: Some(yelang_hir::hir::core::TraitRef {
                path: Res::Def {
                    def_id: deref_trait,
                },
                span: dummy_span(),
            }),
            items: vec![type_item],
            polarity: yelang_hir::hir::core::ImplPolarity::Positive,
            span: dummy_span(),
        });
    }

    // `impl Core { fn val(&self) -> bool { ... } }`
    let bool_hir_ty = hir.alloc_ty(
        hir::Ty::Path {
            res: Res::PrimTy {
                ty: yelang_hir::res::PrimTy::Bool,
            },
            args: vec![],
        },
        dummy_span(),
    );
    let ref_core_ty = hir.alloc_ty(
        hir::Ty::Ref {
            mutability: yelang_ast::Mutability::Immutable,
            ty: core_ty_id,
        },
        dummy_span(),
    );
    let val_sig = yelang_hir::hir::core::FnSig {
        inputs: vec![ref_core_ty],
        output: bool_hir_ty,
        is_async: false,
        is_const: false,
        is_variadic: false,
        abi: None,
        bound_vars: vec![],
    };
    let val_body_value = expr(&mut hir, Expr::Err);
    let val_body = body(&mut hir, vec![], val_body_value);
    let val_impl_item = yelang_hir::hir::core::ImplItem {
        def_id: core_method,
        ident: yelang_ast::Ident::new(symbol(30), dummy_span()),
        kind: yelang_hir::hir::core::ImplItemKind::Fn {
            sig: val_sig,
            body: val_body,
        },
        attrs: vec![],
        span: dummy_span(),
        defaultness: yelang_hir::hir::core::Defaultness::Final,
    };
    hir.impls.push(yelang_hir::hir::core::Impl {
        def_id: core_impl,
        generics: yelang_hir::hir::core::Generics {
            params: vec![],
            where_clause: None,
            span: dummy_span(),
        },
        self_ty: core_ty_id,
        of_trait: None,
        items: vec![val_impl_item],
        polarity: yelang_hir::hir::core::ImplPolarity::Positive,
        span: dummy_span(),
    });

    let mut tcx = TyCtxt::new(hir);
    collect_crate_types(&mut tcx);
    tcx.register_deref_lang_item(deref_trait, target_assoc);
    tcx.populate_solver_caches();

    let wrapper_pat = pat(&mut tcx.crate_hir_mut(), Pat::Wild);
    let receiver = expr(
        &mut tcx.crate_hir_mut(),
        Expr::Path {
            res: local_res(wrapper_pat),
        },
    );
    let call = expr(
        &mut tcx.crate_hir_mut(),
        Expr::MethodCall {
            receiver,
            method: yelang_ast::Ident::new(symbol(30), dummy_span()),
            args: vec![],
            trait_def_id: None,
        },
    );

    let mut fcx = mk_fcx(&tcx);
    let wrapper_ty = fcx
        .tcx
        .item_ty(wrapper_struct)
        .expect("Wrapper should have a type");
    let inner_ty = fcx
        .tcx
        .item_ty(inner_struct)
        .expect("Inner should have a type");
    let core_ty = fcx
        .tcx
        .item_ty(core_struct)
        .expect("Core should have a type");
    fcx.insert_local(wrapper_pat, wrapper_ty);

    let call_ty = check_expr(&mut fcx, call);
    let bool_ty = tcx.interner().mk_ty(Ty::Bool);
    assert_eq!(call_ty, bool_ty);

    let unproven = fcx.prove_obligations();
    assert!(
        unproven.is_empty(),
        "expected chained Deref obligations to be proven, got {:?}",
        unproven
    );

    let adjustments = fcx.results.expr_adjustments(receiver);
    let targets: Vec<TyId> = adjustments
        .iter()
        .filter_map(|a| match a {
            Adjustment::DerefTrait { target, .. } => Some(*target),
            _ => None,
        })
        .collect();
    assert_eq!(targets, vec![inner_ty, core_ty]);
}

#[test]
fn method_not_found_after_autoderef_returns_error() {
    let mut hir = hir_crate();

    let wrapper_struct = def_id(2);

    hir.items.insert(
        wrapper_struct,
        Some(yelang_hir::hir::item::Item {
            def_id: wrapper_struct,
            ident: yelang_ast::Ident::new(symbol(1), dummy_span()),
            kind: yelang_hir::hir::item::ItemKind::Struct {
                data: yelang_hir::hir::adt::VariantData::Unit,
                generics: yelang_hir::hir::core::Generics {
                    params: vec![],
                    where_clause: None,
                    span: dummy_span(),
                },
            },
            vis: yelang_hir::hir::core::Visibility::Public(dummy_span()),
            attrs: vec![],
            span: dummy_span(),
        }),
    );

    let mut tcx = TyCtxt::new(hir);
    collect_crate_types(&mut tcx);

    let wrapper_pat = pat(&mut tcx.crate_hir_mut(), Pat::Wild);
    let receiver = expr(
        &mut tcx.crate_hir_mut(),
        Expr::Path {
            res: local_res(wrapper_pat),
        },
    );
    let call = expr(
        &mut tcx.crate_hir_mut(),
        Expr::MethodCall {
            receiver,
            method: yelang_ast::Ident::new(symbol(99), dummy_span()),
            args: vec![],
            trait_def_id: None,
        },
    );

    let mut fcx = mk_fcx(&tcx);
    let wrapper_ty = fcx
        .tcx
        .item_ty(wrapper_struct)
        .expect("Wrapper should have a type");
    fcx.insert_local(wrapper_pat, wrapper_ty);

    let call_ty = check_expr(&mut fcx, call);
    assert_eq!(call_ty, fcx.mk_error());
}

#[test]
fn trait_solver_writeback_resolves_infer_var() {
    // Set up a trait `Foo` with a single impl `impl Foo for i32`. An obligation
    // `?T: Foo` should resolve `?T` to `i32` via solver substitution writeback.
    let hir = hir_crate();
    let mut tcx = TyCtxt::new(hir);

    let trait_def_id = def_id(10);
    let _impl_def_id = def_id(11);
    let impl_item_def_id = def_id(12);
    let i32_ty = tcx.interner().mk_ty(Ty::Int(IntTy::I32));

    // Trait definition.
    tcx.trait_defs.insert(
        trait_def_id,
        crate::tcx::TraitDefData {
            def_id: trait_def_id,
            ident: yelang_ast::Ident::new(symbol(1), dummy_span()),
            generics: crate::tcx::GenericsData::default(),
            supertraits: Vec::new(),
            items: Vec::new(),
        },
    );

    // Impl block `impl Foo for i32`.
    let impl_id = tcx.impl_defs.push(crate::tcx::ImplDefData {
        id: yelang_arena::Id::new(1),
        def_id: impl_item_def_id,
        trait_ref: Some(yelang_ty::predicate::TraitRef {
            def_id: trait_def_id,
            args: tcx
                .interner()
                .mk_generic_args(&[yelang_ty::generic::GenericArg::Type(i32_ty)]),
        }),
        self_ty: i32_ty,
        generics: crate::tcx::GenericsData::default(),
        items: Vec::new(),
    });
    tcx.trait_impl_index.insert(trait_def_id, vec![impl_id]);
    tcx.populate_solver_caches();

    // Create a body and an inference variable, then emit `?T: Foo`.
    let mut fcx = mk_fcx(&tcx);
    let ty_var = fcx.new_ty_var();
    fcx.emit_trait_obligation(ty_var, trait_def_id);

    let unproven = fcx.prove_obligations();
    assert!(
        unproven.is_empty(),
        "expected obligation to be proven, got {:?}",
        unproven
    );

    // The inference variable should now be resolved to `i32`.
    let resolved = fcx.resolve_ty(ty_var);
    assert_eq!(resolved, i32_ty);
}

// ---------------------------------------------------------------------------
// Field access checking (Phase E)
// ---------------------------------------------------------------------------

#[test]
fn field_struct_named_access() {
    let mut hir = hir_crate();

    let struct_def_id = def_id(2);
    let field_def_id = def_id(10);
    let i32_hir_ty = hir_i32_id(&mut hir);

    hir.items.insert(
        struct_def_id,
        Some(yelang_hir::hir::item::Item {
            def_id: struct_def_id,
            ident: yelang_ast::Ident::new(symbol(1), dummy_span()),
            kind: yelang_hir::hir::item::ItemKind::Struct {
                data: yelang_hir::hir::adt::VariantData::Struct {
                    fields: vec![yelang_hir::hir::adt::FieldDef {
                        def_id: field_def_id,
                        ident: yelang_ast::Ident::new(symbol(10), dummy_span()),
                        ty: i32_hir_ty,
                        span: dummy_span(),
                        vis: yelang_hir::hir::core::Visibility::Public(dummy_span()),
                        attrs: vec![],
                    }],
                },
                generics: yelang_hir::hir::core::Generics {
                    params: vec![],
                    where_clause: None,
                    span: dummy_span(),
                },
            },
            vis: yelang_hir::hir::core::Visibility::Public(dummy_span()),
            attrs: vec![],
            span: dummy_span(),
        }),
    );

    let mut tcx = TyCtxt::new(hir);
    collect_crate_types(&mut tcx);

    let foo_pat = pat(&mut tcx.crate_hir_mut(), Pat::Wild);
    let base = expr(
        &mut tcx.crate_hir_mut(),
        Expr::Path {
            res: local_res(foo_pat),
        },
    );
    let field_expr = expr(
        &mut tcx.crate_hir_mut(),
        Expr::Field {
            expr: base,
            field: yelang_ast::Ident::new(symbol(10), dummy_span()),
        },
    );

    let mut fcx = mk_fcx(&tcx);
    let foo_ty = fcx
        .tcx
        .item_ty(struct_def_id)
        .expect("Foo should have a type");
    fcx.insert_local(foo_pat, foo_ty);

    let ty = check_expr(&mut fcx, field_expr);
    let i32_ty = tcx.interner().mk_ty(Ty::Int(IntTy::I32));
    assert_eq!(ty, i32_ty);
}

#[test]
fn field_struct_missing_is_error() {
    let mut hir = hir_crate();

    let struct_def_id = def_id(2);
    let field_def_id = def_id(10);
    let i32_hir_ty = hir_i32_id(&mut hir);

    hir.items.insert(
        struct_def_id,
        Some(yelang_hir::hir::item::Item {
            def_id: struct_def_id,
            ident: yelang_ast::Ident::new(symbol(1), dummy_span()),
            kind: yelang_hir::hir::item::ItemKind::Struct {
                data: yelang_hir::hir::adt::VariantData::Struct {
                    fields: vec![yelang_hir::hir::adt::FieldDef {
                        def_id: field_def_id,
                        ident: yelang_ast::Ident::new(symbol(10), dummy_span()),
                        ty: i32_hir_ty,
                        span: dummy_span(),
                        vis: yelang_hir::hir::core::Visibility::Public(dummy_span()),
                        attrs: vec![],
                    }],
                },
                generics: yelang_hir::hir::core::Generics {
                    params: vec![],
                    where_clause: None,
                    span: dummy_span(),
                },
            },
            vis: yelang_hir::hir::core::Visibility::Public(dummy_span()),
            attrs: vec![],
            span: dummy_span(),
        }),
    );

    let mut tcx = TyCtxt::new(hir);
    collect_crate_types(&mut tcx);

    let foo_pat = pat(&mut tcx.crate_hir_mut(), Pat::Wild);
    let base = expr(
        &mut tcx.crate_hir_mut(),
        Expr::Path {
            res: local_res(foo_pat),
        },
    );
    let field_expr = expr(
        &mut tcx.crate_hir_mut(),
        Expr::Field {
            expr: base,
            field: yelang_ast::Ident::new(symbol(99), dummy_span()),
        },
    );

    let mut fcx = mk_fcx(&tcx);
    let foo_ty = fcx
        .tcx
        .item_ty(struct_def_id)
        .expect("Foo should have a type");
    fcx.insert_local(foo_pat, foo_ty);

    let ty = check_expr(&mut fcx, field_expr);
    assert_eq!(ty, tcx.interner().mk_ty(Ty::Error));
}

#[test]
fn field_generic_struct_substitutes_params() {
    let mut hir = hir_crate();

    let struct_def_id = def_id(2);
    let t_param = def_id(10);
    let field_def_id = def_id(11);

    let t_ty = hir.alloc_ty(
        hir::Ty::Path {
            res: Res::Def { def_id: t_param },
            args: vec![],
        },
        dummy_span(),
    );

    hir.items.insert(
        struct_def_id,
        Some(yelang_hir::hir::item::Item {
            def_id: struct_def_id,
            ident: yelang_ast::Ident::new(symbol(1), dummy_span()),
            kind: yelang_hir::hir::item::ItemKind::Struct {
                data: yelang_hir::hir::adt::VariantData::Struct {
                    fields: vec![yelang_hir::hir::adt::FieldDef {
                        def_id: field_def_id,
                        ident: yelang_ast::Ident::new(symbol(10), dummy_span()),
                        ty: t_ty,
                        span: dummy_span(),
                        vis: yelang_hir::hir::core::Visibility::Public(dummy_span()),
                        attrs: vec![],
                    }],
                },
                generics: yelang_hir::hir::core::Generics {
                    params: vec![yelang_hir::hir::core::GenericParam::Type {
                        def_id: t_param,
                        name: yelang_ast::Ident::new(symbol(1), dummy_span()),
                        bounds: vec![],
                        default: None,
                        span: dummy_span(),
                    }],
                    where_clause: None,
                    span: dummy_span(),
                },
            },
            vis: yelang_hir::hir::core::Visibility::Public(dummy_span()),
            attrs: vec![],
            span: dummy_span(),
        }),
    );

    let mut tcx = TyCtxt::new(hir);
    collect_crate_types(&mut tcx);

    let foo_pat = pat(&mut tcx.crate_hir_mut(), Pat::Wild);
    let base = expr(
        &mut tcx.crate_hir_mut(),
        Expr::Path {
            res: local_res(foo_pat),
        },
    );
    let field_expr = expr(
        &mut tcx.crate_hir_mut(),
        Expr::Field {
            expr: base,
            field: yelang_ast::Ident::new(symbol(10), dummy_span()),
        },
    );

    let mut fcx = mk_fcx(&tcx);
    let bool_ty = tcx.interner().mk_ty(Ty::Bool);
    let args = tcx
        .interner()
        .mk_generic_args(&[yelang_ty::generic::GenericArg::Type(bool_ty)]);
    let foo_ty = tcx.interner().mk_ty(Ty::Adt(
        yelang_ty::ty::AdtDef {
            def_id: struct_def_id,
        },
        args,
    ));
    fcx.insert_local(foo_pat, foo_ty);

    let ty = check_expr(&mut fcx, field_expr);
    assert_eq!(ty, bool_ty);
}

#[test]
fn field_anon_struct_access() {
    let hir = hir_crate();
    let mut tcx = TyCtxt::new(hir);

    let i32_ty = tcx.interner().mk_ty(Ty::Int(IntTy::I32));
    let bool_ty = tcx.interner().mk_ty(Ty::Bool);
    let anon_ty = tcx
        .interner()
        .mk_ty(Ty::AnonStruct(yelang_ty::ty::AnonStructDef {
            fields: tcx.interner().mk_anon_struct_fields(&[
                yelang_ty::ty::AnonField {
                    name: symbol(1),
                    ty: i32_ty,
                },
                yelang_ty::ty::AnonField {
                    name: symbol(2),
                    ty: bool_ty,
                },
            ]),
        }));

    let pat = pat(&mut tcx.crate_hir_mut(), Pat::Wild);
    let base = expr(
        &mut tcx.crate_hir_mut(),
        Expr::Path {
            res: local_res(pat),
        },
    );
    let field_expr = expr(
        &mut tcx.crate_hir_mut(),
        Expr::Field {
            expr: base,
            field: yelang_ast::Ident::new(symbol(2), dummy_span()),
        },
    );

    let mut fcx = mk_fcx(&tcx);
    fcx.insert_local(pat, anon_ty);

    let ty = check_expr(&mut fcx, field_expr);
    assert_eq!(ty, bool_ty);
}

#[test]
fn field_through_reference() {
    let mut hir = hir_crate();

    let struct_def_id = def_id(2);
    let field_def_id = def_id(10);
    let i32_hir_ty = hir_i32_id(&mut hir);

    hir.items.insert(
        struct_def_id,
        Some(yelang_hir::hir::item::Item {
            def_id: struct_def_id,
            ident: yelang_ast::Ident::new(symbol(1), dummy_span()),
            kind: yelang_hir::hir::item::ItemKind::Struct {
                data: yelang_hir::hir::adt::VariantData::Struct {
                    fields: vec![yelang_hir::hir::adt::FieldDef {
                        def_id: field_def_id,
                        ident: yelang_ast::Ident::new(symbol(10), dummy_span()),
                        ty: i32_hir_ty,
                        span: dummy_span(),
                        vis: yelang_hir::hir::core::Visibility::Public(dummy_span()),
                        attrs: vec![],
                    }],
                },
                generics: yelang_hir::hir::core::Generics {
                    params: vec![],
                    where_clause: None,
                    span: dummy_span(),
                },
            },
            vis: yelang_hir::hir::core::Visibility::Public(dummy_span()),
            attrs: vec![],
            span: dummy_span(),
        }),
    );

    let mut tcx = TyCtxt::new(hir);
    collect_crate_types(&mut tcx);

    let foo_pat = pat(&mut tcx.crate_hir_mut(), Pat::Wild);
    let base = expr(
        &mut tcx.crate_hir_mut(),
        Expr::Path {
            res: local_res(foo_pat),
        },
    );
    let field_expr = expr(
        &mut tcx.crate_hir_mut(),
        Expr::Field {
            expr: base,
            field: yelang_ast::Ident::new(symbol(10), dummy_span()),
        },
    );

    let mut fcx = mk_fcx(&tcx);
    let foo_ty = fcx
        .tcx
        .item_ty(struct_def_id)
        .expect("Foo should have a type");
    let ref_foo_ty = tcx.interner().mk_ty(Ty::Ref(foo_ty, Mutability::Not));
    fcx.insert_local(foo_pat, ref_foo_ty);

    let ty = check_expr(&mut fcx, field_expr);
    let i32_ty = tcx.interner().mk_ty(Ty::Int(IntTy::I32));
    assert_eq!(ty, i32_ty);

    let adjustments = fcx.results.expr_adjustments(base);
    assert_eq!(adjustments, &[Adjustment::Deref]);
}

#[test]
fn field_through_deref_trait() {
    let mut hir = hir_crate();

    let inner_struct = def_id(2);
    let wrapper_struct = def_id(3);
    let deref_trait = def_id(4);
    let target_assoc = def_id(5);
    let deref_impl = def_id(6);
    let target_impl_item = def_id(7);
    let field_def_id = def_id(8);

    let inner_ty_id = hir.alloc_ty(
        hir::Ty::Path {
            res: Res::Def {
                def_id: inner_struct,
            },
            args: vec![],
        },
        dummy_span(),
    );
    let wrapper_ty_id = hir.alloc_ty(
        hir::Ty::Path {
            res: Res::Def {
                def_id: wrapper_struct,
            },
            args: vec![],
        },
        dummy_span(),
    );

    let i32_hir_ty = hir_i32_id(&mut hir);

    // `struct Inner { x: i32 }`
    hir.items.insert(
        inner_struct,
        Some(yelang_hir::hir::item::Item {
            def_id: inner_struct,
            ident: yelang_ast::Ident::new(symbol(1), dummy_span()),
            kind: yelang_hir::hir::item::ItemKind::Struct {
                data: yelang_hir::hir::adt::VariantData::Struct {
                    fields: vec![yelang_hir::hir::adt::FieldDef {
                        def_id: field_def_id,
                        ident: yelang_ast::Ident::new(symbol(10), dummy_span()),
                        ty: i32_hir_ty,
                        span: dummy_span(),
                        vis: yelang_hir::hir::core::Visibility::Public(dummy_span()),
                        attrs: vec![],
                    }],
                },
                generics: yelang_hir::hir::core::Generics {
                    params: vec![],
                    where_clause: None,
                    span: dummy_span(),
                },
            },
            vis: yelang_hir::hir::core::Visibility::Public(dummy_span()),
            attrs: vec![],
            span: dummy_span(),
        }),
    );

    // `struct Wrapper;`
    hir.items.insert(
        wrapper_struct,
        Some(yelang_hir::hir::item::Item {
            def_id: wrapper_struct,
            ident: yelang_ast::Ident::new(symbol(2), dummy_span()),
            kind: yelang_hir::hir::item::ItemKind::Struct {
                data: yelang_hir::hir::adt::VariantData::Unit,
                generics: yelang_hir::hir::core::Generics {
                    params: vec![],
                    where_clause: None,
                    span: dummy_span(),
                },
            },
            vis: yelang_hir::hir::core::Visibility::Public(dummy_span()),
            attrs: vec![],
            span: dummy_span(),
        }),
    );

    // `trait Deref { type Target; }`
    let target_trait_item = yelang_hir::hir::core::TraitItem {
        def_id: target_assoc,
        ident: yelang_ast::Ident::new(symbol(21), dummy_span()),
        kind: yelang_hir::hir::core::TraitItemKind::Type {
            bounds: vec![],
            default: None,
        },
        attrs: vec![],
        span: dummy_span(),
    };
    hir.traits.insert(
        deref_trait,
        Some(yelang_hir::hir::core::Trait {
            name: yelang_ast::Ident::new(symbol(20), dummy_span()),
            generics: yelang_hir::hir::core::Generics {
                params: vec![],
                where_clause: None,
                span: dummy_span(),
            },
            super_traits: vec![],
            items: vec![target_trait_item],
            span: dummy_span(),
        }),
    );

    // `impl Deref for Wrapper { type Target = Inner; }`
    let target_impl_assoc = yelang_hir::hir::core::ImplItem {
        def_id: target_impl_item,
        ident: yelang_ast::Ident::new(symbol(21), dummy_span()),
        kind: yelang_hir::hir::core::ImplItemKind::Type { ty: inner_ty_id },
        attrs: vec![],
        span: dummy_span(),
        defaultness: yelang_hir::hir::core::Defaultness::Final,
    };
    hir.impls.push(yelang_hir::hir::core::Impl {
        def_id: deref_impl,
        generics: yelang_hir::hir::core::Generics {
            params: vec![],
            where_clause: None,
            span: dummy_span(),
        },
        self_ty: wrapper_ty_id,
        of_trait: Some(yelang_hir::hir::core::TraitRef {
            path: Res::Def {
                def_id: deref_trait,
            },
            span: dummy_span(),
        }),
        items: vec![target_impl_assoc],
        polarity: yelang_hir::hir::core::ImplPolarity::Positive,
        span: dummy_span(),
    });

    let mut tcx = TyCtxt::new(hir);
    collect_crate_types(&mut tcx);
    tcx.register_deref_lang_item(deref_trait, target_assoc);
    tcx.populate_solver_caches();

    let wrapper_pat = pat(&mut tcx.crate_hir_mut(), Pat::Wild);
    let base = expr(
        &mut tcx.crate_hir_mut(),
        Expr::Path {
            res: local_res(wrapper_pat),
        },
    );
    let field_expr = expr(
        &mut tcx.crate_hir_mut(),
        Expr::Field {
            expr: base,
            field: yelang_ast::Ident::new(symbol(10), dummy_span()),
        },
    );

    let mut fcx = mk_fcx(&tcx);
    let wrapper_ty = fcx
        .tcx
        .item_ty(wrapper_struct)
        .expect("Wrapper should have a type");
    let inner_ty = fcx
        .tcx
        .item_ty(inner_struct)
        .expect("Inner should have a type");
    fcx.insert_local(wrapper_pat, wrapper_ty);

    let ty = check_expr(&mut fcx, field_expr);
    let i32_ty = tcx.interner().mk_ty(Ty::Int(IntTy::I32));
    assert_eq!(ty, i32_ty);

    let unproven = fcx.prove_obligations();
    assert!(
        unproven.is_empty(),
        "expected Deref obligations to be proven, got {:?}",
        unproven
    );

    let adjustments = fcx.results.expr_adjustments(base);
    assert!(
        adjustments.iter().any(|a| matches!(
            a,
            Adjustment::DerefTrait { target, .. } if *target == inner_ty
        )),
        "expected a DerefTrait adjustment to Inner, got {:?}",
        adjustments
    );
}

#[test]
fn return_type_infer_from_body() {
    let mut hir = hir_crate();

    let fn_def_id = def_id(2);
    let infer_return = hir.alloc_ty(hir::Ty::Infer, dummy_span());
    let _i32_hir_ty = hir_i32_id(&mut hir);

    let body_value = expr(
        &mut hir,
        Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(42),
                suffix: None,
            }),
        },
    );
    let fn_body = body(&mut hir, vec![], body_value);

    hir.items.insert(
        fn_def_id,
        Some(yelang_hir::hir::item::Item {
            def_id: fn_def_id,
            ident: yelang_ast::Ident::new(symbol(1), dummy_span()),
            kind: yelang_hir::hir::item::ItemKind::Fn {
                sig: yelang_hir::hir::core::FnSig {
                    inputs: vec![],
                    output: infer_return,
                    is_async: false,
                    is_const: false,
                    is_variadic: false,
                    abi: None,
                    bound_vars: vec![],
                },
                body: fn_body,
                generics: yelang_hir::hir::core::Generics {
                    params: vec![],
                    where_clause: None,
                    span: dummy_span(),
                },
            },
            vis: yelang_hir::hir::core::Visibility::Public(dummy_span()),
            attrs: vec![],
            span: dummy_span(),
        }),
    );

    let mut tcx = TyCtxt::new(hir);
    collect_crate_types(&mut tcx);

    let mut fcx = FnCtxt::new(
        &tcx,
        fn_def_id,
        tcx.interner()
            .mk_ty(Ty::Tuple(yelang_ty::list::List::empty())),
    );
    check_body(&mut fcx, fn_body);

    let i32_ty = tcx.interner().mk_ty(Ty::Int(IntTy::I32));
    let resolved_return = fcx.resolve_ty(fcx.return_ty);
    assert_eq!(resolved_return, i32_ty);
}

// ---------------------------------------------------------------------------
// Ty -> Ty lowering edge cases (Phase F)
// ---------------------------------------------------------------------------

#[test]
fn syntax_ty_infer_in_body_is_fresh_var() {
    let hir = hir_crate();
    let mut tcx = TyCtxt::new(hir);
    let ty_id = tcx.crate_hir_mut().alloc_ty(hir::Ty::Infer, dummy_span());

    let mut fcx = mk_fcx(&tcx);
    let ty = lower_hir_ty_id(ty_id, &mut fcx);
    assert!(matches!(
        tcx.interner().ty(ty),
        Ty::Infer(yelang_ty::ty::InferTy::TyVar(_))
    ));
}

#[test]
fn syntax_ty_missing_in_body_is_fresh_var() {
    let hir = hir_crate();
    let mut tcx = TyCtxt::new(hir);
    let ty_id = tcx.crate_hir_mut().alloc_ty(hir::Ty::Missing, dummy_span());

    let mut fcx = mk_fcx(&tcx);
    let ty = lower_hir_ty_id(ty_id, &mut fcx);
    assert!(matches!(
        tcx.interner().ty(ty),
        Ty::Infer(yelang_ty::ty::InferTy::TyVar(_))
    ));
}

#[test]
fn syntax_ty_typeof_in_body_lowers_to_expr_type() {
    let hir = hir_crate();
    let mut tcx = TyCtxt::new(hir);
    let inner = expr(
        &mut tcx.crate_hir_mut(),
        Expr::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: symbol(1),
                suffix: None,
            }),
        },
    );
    let ty_id = tcx
        .crate_hir_mut()
        .alloc_ty(hir::Ty::TypeOf { expr: inner }, dummy_span());

    let mut fcx = mk_fcx(&tcx);
    let ty = lower_hir_ty_id(ty_id, &mut fcx);
    assert!(matches!(
        tcx.interner().ty(ty),
        Ty::Infer(yelang_ty::ty::InferTy::IntVar(_))
    ));
}

#[test]
fn syntax_ty_impl_trait_lowers_to_alias() {
    let hir = hir_crate();
    let mut tcx = TyCtxt::new(hir);
    let trait_def_id = def_id(10);
    let ty_id = tcx.crate_hir_mut().alloc_ty(
        hir::Ty::ImplTrait {
            path: Res::Def {
                def_id: trait_def_id,
            },
        },
        dummy_span(),
    );

    let mut fcx = mk_fcx(&tcx);
    let ty = lower_hir_ty_id(ty_id, &mut fcx);
    match tcx.interner().ty(ty) {
        Ty::Alias(alias) => assert_eq!(alias.def_id, trait_def_id),
        _ => panic!("expected Alias for impl Trait"),
    }
}
