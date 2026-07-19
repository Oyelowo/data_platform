/*! Writeback of inferred types.
 *
 * After inference is complete, writes the final inferred types
 * back into the type tables, resolving any remaining inference
 * variables and applying default fallback (int -> i32, float -> f64).
 */

use yelang_hir::ids::{ExprId, PatId};
use yelang_ty::primitive::{FloatTy, IntTy};
use yelang_ty::ty::{InferTy, Ty, TyId};

use crate::fn_ctxt::FnCtxt;

/// Write inferred types back to the results tables.
pub fn writeback_types(fcx: &mut FnCtxt<'_>) {
    // Resolve expression types
    let expr_entries: Vec<(ExprId, TyId)> = fcx
        .results
        .expr_types
        .iter()
        .map(|(&id, &ty)| (id, ty))
        .collect();
    for (id, ty) in expr_entries {
        let resolved = resolve_with_fallback(fcx, ty);
        fcx.results.expr_types.insert(id, resolved);
    }

    // Resolve pattern types
    let pat_entries: Vec<(PatId, TyId)> = fcx
        .results
        .pat_types
        .iter()
        .map(|(&id, &ty)| (id, ty))
        .collect();
    for (id, ty) in pat_entries {
        let resolved = resolve_with_fallback(fcx, ty);
        fcx.results.pat_types.insert(id, resolved);
    }

    // Resolve local types
    let local_entries: Vec<(PatId, TyId)> = fcx
        .results
        .local_types
        .iter()
        .map(|(&id, &ty)| (id, ty))
        .collect();
    for (id, ty) in local_entries {
        let resolved = resolve_with_fallback(fcx, ty);
        fcx.results.local_types.insert(id, resolved);
    }
}

/// Resolve a type, applying fallback for unresolved int/float variables.
fn resolve_with_fallback(fcx: &mut FnCtxt<'_>, ty: TyId) -> TyId {
    let interner = fcx.tcx.interner();
    match interner.ty(ty) {
        Ty::Infer(InferTy::IntVar(vid)) => {
            // Integer fallback: i32. Also commit the fallback to the inference
            // tables so that other references to the same variable resolve
            // consistently (e.g. an inferred return type).
            let root = fcx.infer.find_int_var(vid);
            let _ = fcx.infer.set_int_var(root, IntTy::I32);
            fcx.mk_int(IntTy::I32)
        }
        Ty::Infer(InferTy::FloatVar(vid)) => {
            // Float fallback: f64.
            let root = fcx.infer.find_float_var(vid);
            let _ = fcx.infer.set_float_var(root, FloatTy::F64);
            fcx.mk_float(FloatTy::F64)
        }
        Ty::Infer(InferTy::TyVar(_)) => {
            // General type variable: try to resolve. If it resolves to another
            // inference variable (e.g. a TyVar unified with an IntVar because
            // of an unannotated closure parameter), apply the corresponding
            // fallback. Otherwise report an error.
            let resolved = fcx.resolve_ty(ty);
            match interner.ty(resolved) {
                Ty::Infer(InferTy::TyVar(_)) => fcx.mk_error(),
                Ty::Infer(InferTy::IntVar(_)) => resolve_with_fallback(fcx, resolved),
                Ty::Infer(InferTy::FloatVar(_)) => resolve_with_fallback(fcx, resolved),
                _ => resolved,
            }
        }
        _ => fcx.resolve_ty(ty),
    }
}
