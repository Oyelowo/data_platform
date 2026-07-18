/*! Pattern type checking.
 *
 * Checks that patterns match the expected type and extracts
 * bound variable types.
 */

use yelang_hir::hir::pat::Pat;
use yelang_hir::ids::PatId;
use yelang_ty::ty::TyId;

use crate::fn_ctxt::FnCtxt;

/// Check a pattern against an expected type.
pub fn check_pat(fcx: &mut FnCtxt<'_>, pat_id: PatId, expected_ty: TyId) {
    let pat = fcx
        .tcx.crate_hir()
        .pat(pat_id)
        .expect("PatId should be valid")
        .clone();
    match &pat {
        Pat::Wild => {
            fcx.record_pat_ty(pat_id, expected_ty);
        }
        Pat::Binding {
            name: _, subpat, ..
        } => {
            fcx.insert_local(pat_id, expected_ty);
            fcx.record_pat_ty(pat_id, expected_ty);
            if let Some(sub) = subpat {
                check_pat(fcx, *sub, expected_ty);
            }
        }
        Pat::Struct {
            res: _,
            fields,
            rest,
        } => {
            // TODO: check field types against struct definition
            for field in fields {
                check_pat(fcx, field.pat, expected_ty);
            }
            let _ = rest;
            fcx.record_pat_ty(pat_id, expected_ty);
        }
        Pat::Tuple { pats } => {
            // TODO: destructure tuple type
            for p in pats {
                check_pat(fcx, *p, expected_ty);
            }
            fcx.record_pat_ty(pat_id, expected_ty);
        }
        Pat::TupleStruct { res: _, pats } => {
            // TODO: check against enum/struct variant
            for p in pats {
                check_pat(fcx, *p, expected_ty);
            }
            fcx.record_pat_ty(pat_id, expected_ty);
        }
        Pat::Path { res: _ } => {
            // TODO: check path resolution against expected type
            fcx.record_pat_ty(pat_id, expected_ty);
        }
        Pat::Lit { lit: _ } => {
            // TODO: check literal type against expected type
            fcx.record_pat_ty(pat_id, expected_ty);
        }
        Pat::Range {
            start,
            end,
            end_inclusive: _,
        } => {
            if let Some(s) = start {
                check_pat(fcx, *s, expected_ty);
            }
            if let Some(e) = end {
                check_pat(fcx, *e, expected_ty);
            }
            fcx.record_pat_ty(pat_id, expected_ty);
        }
        Pat::Or { pats } => {
            for p in pats {
                check_pat(fcx, *p, expected_ty);
            }
            fcx.record_pat_ty(pat_id, expected_ty);
        }
        Pat::Slice {
            prefix,
            middle,
            suffix,
        } => {
            for p in prefix {
                check_pat(fcx, *p, expected_ty);
            }
            if let Some(m) = middle {
                check_pat(fcx, *m, expected_ty);
            }
            for p in suffix {
                check_pat(fcx, *p, expected_ty);
            }
            fcx.record_pat_ty(pat_id, expected_ty);
        }
        Pat::Ref { pat, .. } => {
            // TODO: check that expected_ty is a reference and deref for inner check
            check_pat(fcx, *pat, expected_ty);
            fcx.record_pat_ty(pat_id, expected_ty);
        }
        Pat::Rest { .. } => {
            // Rest pattern does not bind a value on its own.
            fcx.record_pat_ty(pat_id, expected_ty);
        }
        Pat::Err => {
            fcx.record_pat_ty(pat_id, fcx.mk_error());
        }
    }
}
