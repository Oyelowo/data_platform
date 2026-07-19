/*! Compiler-known intercepts for array/collection operations.
 *
 * These are not ordinary stdlib methods; they are recognized by the type checker
 * and either lowered to primitive operations or proved via the trait solver.
 * This keeps the prelude surface small while giving `Array<T>` ergonomic
 * methods like `len`, `is_empty`, `any`, and `all`.
 */

use yelang_hir::ids::ExprId;
use yelang_hir::res::Res;
use yelang_interner::Symbol;
use yelang_lexer::Span;
use yelang_resolve::lang_items::LangItem;
use yelang_ty::primitive::UintTy;
use yelang_ty::ty::{Ty, TyId};

use crate::check::{check_closure_with_expected, check_expr, expr_span};
use crate::fn_ctxt::FnCtxt;
use crate::typeck_results::MethodResolution;

/// If `func` is the prelude `len` or `count` function, type-check the call as a
/// builtin and return the result type. Returns `None` when the call is not an
/// array-length builtin.
pub fn try_check_len_or_count_call(
    fcx: &mut FnCtxt<'_>,
    func: ExprId,
    args: &[ExprId],
) -> Option<TyId> {
    let def_id = builtin_fn_def_id(fcx, func)?;
    let is_len_or_count = Some(def_id) == fcx.tcx.lang_item(LangItem::Len)
        || Some(def_id) == fcx.tcx.lang_item(LangItem::Count);
    if !is_len_or_count {
        return None;
    }

    let span = expr_span(fcx, func);
    if args.len() != 1 {
        fcx.report_type_error(
            span,
            yelang_infer::error::TypeError::ArgCount {
                expected: 1,
                found: args.len(),
            },
        );
        return Some(fcx.mk_error());
    }

    let arg = args[0];
    let arg_ty = check_expr(fcx, arg);
    fcx.expect_array(expr_span(fcx, arg), arg_ty);

    Some(fcx.mk_uint(UintTy::Usize))
}

/// If `receiver.method(args)` is a builtin array method (`is_empty`, `any`,
/// `all`), type-check it and return the result type. Returns `None` for ordinary
/// method calls.
pub fn try_check_array_method_call(
    fcx: &mut FnCtxt<'_>,
    receiver: ExprId,
    method: Symbol,
    args: &[ExprId],
) -> Option<TyId> {
    let method_name = fcx.tcx.resolve_symbol(method).unwrap_or("_");

    match method_name {
        "is_empty" => try_check_is_empty(fcx, receiver, args),
        "any" | "all" => try_check_any_all(fcx, receiver, method_name, args),
        _ => None,
    }
}

fn try_check_is_empty(
    fcx: &mut FnCtxt<'_>,
    receiver: ExprId,
    args: &[ExprId],
) -> Option<TyId> {
    if !args.is_empty() {
        return None;
    }
    let receiver_ty = check_expr(fcx, receiver);
    fcx.expect_array(expr_span(fcx, receiver), receiver_ty);
    fcx.results.record_method_resolution(
        receiver,
        MethodResolution {
            trait_def_id: None,
            method_def_id: None,
            impl_def_id: None,
        },
    );
    Some(fcx.mk_bool())
}

fn try_check_any_all(
    fcx: &mut FnCtxt<'_>,
    receiver: ExprId,
    method_name: &str,
    args: &[ExprId],
) -> Option<TyId> {
    if args.len() != 1 {
        return None;
    }

    let receiver_ty = check_expr(fcx, receiver);
    let elem_ty = fcx.expect_array(expr_span(fcx, receiver), receiver_ty);

    let predicate = args[0];
    let pred_ty = if let Some(yelang_hir::hir::expr::Expr::Closure { body, .. }) =
        fcx.tcx.crate_hir().expr(predicate)
    {
        // Closures passed to `any`/`all` can infer their parameter type from
        // the array element type.
        check_closure_with_expected(fcx, *body, &[elem_ty])
    } else {
        check_expr(fcx, predicate)
    };
    let interner = fcx.tcx.interner();

    // The predicate must be callable with the element type and return bool.
    match interner.ty(pred_ty) {
        Ty::FnPtr(sig) => {
            let inputs = &sig.sig.inputs;
            if inputs.len() != 1 {
                let span = expr_span(fcx, predicate);
                fcx.report_type_error(
                    span,
                    yelang_infer::error::TypeError::Custom(format!(
                        "expected a single-argument predicate for `{}`",
                        method_name
                    )),
                );
                return Some(fcx.mk_bool());
            }
            let expected_input = match inputs.iter().next().unwrap() {
                yelang_ty::generic::GenericArg::Type(ty) => *ty,
                _ => {
                    let span = expr_span(fcx, predicate);
                    fcx.report_type_error(
                        span,
                        yelang_infer::error::TypeError::Custom(format!(
                            "expected a type argument for predicate `{}`",
                            method_name
                        )),
                    );
                    return Some(fcx.mk_bool());
                }
            };
            let span = expr_span(fcx, predicate);
            if fcx.eq(expected_input, elem_ty).is_err() {
                fcx.report_mismatch(span, elem_ty, expected_input);
            }
            if fcx.eq(sig.sig.output, fcx.mk_bool()).is_err() {
                fcx.report_mismatch(span, fcx.mk_bool(), sig.sig.output);
            }
        }
        _ => {
            let span = expr_span(fcx, predicate);
            fcx.report_type_error(
                span,
                yelang_infer::error::TypeError::Custom(format!(
                    "expected a predicate closure for `{}`, found `{}`",
                    method_name,
                    crate::fn_ctxt::format_ty(fcx.tcx, pred_ty)
                )),
            );
        }
    }

    fcx.results.record_method_resolution(
        receiver,
        MethodResolution {
            trait_def_id: None,
            method_def_id: None,
            impl_def_id: None,
        },
    );
    Some(fcx.mk_bool())
}

fn builtin_fn_def_id(fcx: &FnCtxt<'_>, func: ExprId) -> Option<yelang_arena::DefId> {
    let expr = fcx.tcx.crate_hir().expr(func)?;
    match &expr {
        yelang_hir::hir::expr::Expr::Path { res: Res::Def { def_id } } => Some(*def_id),
        _ => None,
    }
}

/// Convenience re-export of `expr_span` so callers do not need to import it
/// separately. Kept internal to this module.
#[allow(dead_code)]
fn span_for(fcx: &FnCtxt<'_>, expr: ExprId) -> Span {
    expr_span(fcx, expr)
}
