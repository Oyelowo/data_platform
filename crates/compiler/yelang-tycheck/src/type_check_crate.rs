/*! Crate-level type-checking entry point.
 *
 * `type_check_crate` collects item signatures, checks every function body, and
 * returns any diagnostics produced.
 */

use yelang_arena::DefId;
use yelang_hir::hir::item::ItemKind;
use yelang_lexer::Span;
use yelang_ty::ty::Ty;

use crate::check::check_body;
use crate::collector;
use crate::diagnostics::{Diagnostic, Severity};
use crate::fn_ctxt::FnCtxt;
use crate::tcx::TyCtxt;

/// Type-check every body in the crate and return diagnostics.
pub fn type_check_crate(tcx: &mut TyCtxt) -> Vec<Diagnostic> {
    collector::collect_crate_types(tcx);

    let mut all_diagnostics = Vec::new();

    // Collect function bodies to check. We copy the IDs first so we don't hold
    // a borrow across mutation of `tcx`.
    let fn_bodies: Vec<(DefId, Span)> = tcx
        .crate_hir()
        .items
        .iter_enumerated()
        .filter_map(|(def_id, item)| {
            let item = item.as_ref()?;
            match &item.kind {
                ItemKind::Fn { .. } => Some((def_id, item.span)),
                _ => None,
            }
        })
        .collect();

    for (def_id, _item_span) in fn_bodies {
        let Some(poly_sig) = tcx.fn_sig(def_id) else {
            continue;
        };
        let return_ty = poly_sig.sig.output;
        let mut fcx = FnCtxt::new(tcx, def_id, return_ty);

        // Find the body id again from the item we already filtered.
        let body_id = tcx
            .crate_hir()
            .items
            .get(def_id)
            .and_then(|i| i.as_ref())
            .and_then(|i| match &i.kind {
                ItemKind::Fn { body, .. } => Some(*body),
                _ => None,
            });

        if let Some(body_id) = body_id {
            check_body(&mut fcx, body_id);
        }

        // Convert FnCtxt errors into diagnostics.
        for (span, err) in &fcx.errors {
            all_diagnostics.push(Diagnostic::from_type_error(*span, err, tcx));
        }

        // Report any unresolved inference variables as errors.
        for (&expr_id, &ty) in &fcx.results.expr_types {
            if matches!(tcx.interner().ty(ty), Ty::Infer(_)) {
                let span = tcx.crate_hir().expr_span(expr_id);
                all_diagnostics.push(Diagnostic {
                    span,
                    message: "type annotations needed".to_string(),
                    severity: Severity::Error,
                });
            }
        }
    }

    all_diagnostics
}
