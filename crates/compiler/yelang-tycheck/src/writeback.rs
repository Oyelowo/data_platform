/*! Writeback of inferred types.
 *
 * After inference is complete, writes the final inferred types
 * back into the type tables, resolving any remaining inference
 * variables and applying default fallback (int -> i32, float -> f64).
 */

use yelang_hir::ids::{ExprId, PatId};
use yelang_ty::fold::{TypeFoldable, TypeFolder, TypeSuperFoldable};
use yelang_ty::interner::Interner;
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
    let mut folder = ResolveFolder { fcx };
    ty.fold_with(&mut folder)
}

struct ResolveFolder<'a, 'b> {
    fcx: &'a mut FnCtxt<'b>,
}

impl TypeFolder for ResolveFolder<'_, '_> {
    fn interner(&self) -> &Interner {
        self.fcx.tcx.interner()
    }

    fn fold_ty(&mut self, ty: TyId) -> TyId {
        let interner = self.interner();
        match interner.ty(ty) {
            Ty::Infer(InferTy::TyVar(vid)) => {
                let root = self.fcx.infer.find_ty_var(vid);
                match self.fcx.infer.probe_ty_var(root).clone() {
                    yelang_infer::type_variable::TypeVarValue::Known(known) => known.fold_with(self),
                    yelang_infer::type_variable::TypeVarValue::Unknown => {
                        // Unresolved general type variable: report as error.
                        self.fcx.mk_error()
                    }
                }
            }
            Ty::Infer(InferTy::IntVar(vid)) => {
                let root = self.fcx.infer.find_int_var(vid);
                match self.fcx.infer.probe_int_var(root).clone() {
                    yelang_infer::type_variable::IntVarValue::Known(it) => match it {
                        yelang_ty::primitive::IntegerTy::Signed(it) => self.fcx.mk_int(it),
                        yelang_ty::primitive::IntegerTy::Unsigned(ut) => self.fcx.mk_uint(ut),
                    },
                    yelang_infer::type_variable::IntVarValue::Unknown => {
                        // Integer fallback: i32.
                        let _ = self.fcx.infer.set_int_var(root, IntTy::I32);
                        self.fcx.mk_int(IntTy::I32)
                    }
                }
            }
            Ty::Infer(InferTy::FloatVar(vid)) => {
                let root = self.fcx.infer.find_float_var(vid);
                match self.fcx.infer.probe_float_var(root).clone() {
                    yelang_infer::type_variable::FloatVarValue::Known(ft) => self.fcx.mk_float(ft),
                    yelang_infer::type_variable::FloatVarValue::Unknown => {
                        // Float fallback: f64.
                        let _ = self.fcx.infer.set_float_var(root, FloatTy::F64);
                        self.fcx.mk_float(FloatTy::F64)
                    }
                }
            }
            _ => ty.super_fold_with(self),
        }
    }
}
