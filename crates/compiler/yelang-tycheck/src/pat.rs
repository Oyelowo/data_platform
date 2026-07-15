/*! Pattern type checking.
 *
 * Checks that patterns match the expected type and extracts
 * bound variable types.
 */

use yelang_hir::hir_pat::{Pat, PatKind};
use yelang_ty::ty::Ty;

use crate::fn_ctxt::FnCtxt;

/// Check a pattern against an expected type.
pub fn check_pat<'tcx>(fcx: &mut FnCtxt<'tcx>, pat: &Pat, expected_ty: Ty<'tcx>) {
    match &pat.kind {
        PatKind::Wild => {
            fcx.record_pat_ty(pat.hir_id, expected_ty);
        }
        PatKind::Binding { name: _, subpat, .. } => {
            fcx.insert_local(pat.hir_id, expected_ty);
            fcx.record_pat_ty(pat.hir_id, expected_ty);
            if let Some(sub) = subpat {
                check_pat(fcx, sub, expected_ty);
            }
        }
        PatKind::Struct { res: _, fields, rest } => {
            // TODO: check field types against struct definition
            for field in fields {
                check_pat(fcx, &field.pat, expected_ty);
            }
            let _ = rest;
            fcx.record_pat_ty(pat.hir_id, expected_ty);
        }
        PatKind::Tuple { pats } => {
            // TODO: destructure tuple type
            for p in pats {
                check_pat(fcx, p, expected_ty);
            }
            fcx.record_pat_ty(pat.hir_id, expected_ty);
        }
        PatKind::TupleStruct { res: _, pats } => {
            // TODO: check against enum/struct variant
            for p in pats {
                check_pat(fcx, p, expected_ty);
            }
            fcx.record_pat_ty(pat.hir_id, expected_ty);
        }
        PatKind::Path { res: _ } => {
            // TODO: check path resolution against expected type
            fcx.record_pat_ty(pat.hir_id, expected_ty);
        }
        PatKind::Lit { lit: _ } => {
            // TODO: check literal type against expected type
            fcx.record_pat_ty(pat.hir_id, expected_ty);
        }
        PatKind::Range { start, end, end_inclusive: _ } => {
            if let Some(s) = start {
                check_pat(fcx, s, expected_ty);
            }
            if let Some(e) = end {
                check_pat(fcx, e, expected_ty);
            }
            fcx.record_pat_ty(pat.hir_id, expected_ty);
        }
        PatKind::Or { pats } => {
            for p in pats {
                check_pat(fcx, p, expected_ty);
            }
            fcx.record_pat_ty(pat.hir_id, expected_ty);
        }
        PatKind::Slice { prefix, middle, suffix } => {
            for p in prefix {
                check_pat(fcx, p, expected_ty);
            }
            if let Some(m) = middle {
                check_pat(fcx, m, expected_ty);
            }
            for p in suffix {
                check_pat(fcx, p, expected_ty);
            }
            fcx.record_pat_ty(pat.hir_id, expected_ty);
        }
        PatKind::Err => {
            fcx.record_pat_ty(pat.hir_id, fcx.mk_error());
        }
    }
}
