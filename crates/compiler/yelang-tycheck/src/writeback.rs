/*! Writeback of inferred types.
 *
 * After inference is complete, writes the final inferred types
 * back into the type tables, resolving any remaining inference
 * variables and applying default fallback (int -> i32, float -> f64).
 */

use yelang_ty::primitive::{FloatTy, IntTy};
use yelang_ty::ty::{InferTy, Ty, TyKind};
use yelang_arena::HirId;

use crate::fn_ctxt::FnCtxt;

/// Write inferred types back to the results tables.
pub fn writeback_types<'tcx>(fcx: &mut FnCtxt<'tcx>) {
    // Resolve expression types
    let expr_entries: Vec<(HirId, Ty<'tcx>)> = fcx
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
    let pat_entries: Vec<(HirId, Ty<'tcx>)> = fcx
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
    let local_entries: Vec<(HirId, Ty<'tcx>)> = fcx
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
fn resolve_with_fallback<'tcx>(fcx: &mut FnCtxt<'tcx>, ty: Ty<'tcx>) -> Ty<'tcx> {
    match ty.kind() {
        TyKind::Infer(InferTy::IntVar(_)) => {
            // Integer fallback: i32
            fcx.mk_int(IntTy::I32)
        }
        TyKind::Infer(InferTy::FloatVar(_)) => {
            // Float fallback: f64
            fcx.mk_float(FloatTy::F64)
        }
        TyKind::Infer(InferTy::TyVar(_)) => {
            // General type variable: try to resolve, otherwise error
            let resolved = fcx.resolve_ty(ty);
            if matches!(resolved.kind(), TyKind::Infer(InferTy::TyVar(_))) {
                fcx.mk_error()
            } else {
                resolved
            }
        }
        _ => fcx.resolve_ty(ty),
    }
}
