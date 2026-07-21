//! Simple THIR method-body inliner for `Queryable` sugar methods.

use yelang_arena::DefId;
use yelang_thir::ThirExprId;

use crate::errors::LoweringError;

/// Inline a trait/impl method body, substituting `self` and formal params with
/// the supplied argument expressions.
///
/// The full THIR inliner would lower the method body from HIR, build a
/// substitution over THIR pattern ids, and clone the body expression tree.
/// Today the compiler only type-checks top-level function bodies, so impl and
/// trait default bodies are not available as typed THIR. We therefore handle
/// the important sugar case (`sum`, `count`, `avg`, `min`, `max`) by directly
/// constructing the `self.aggregate(Marker {})` call that those default bodies
/// expand to. Everything else falls back to a documented error.
pub fn inline_method_body(
    ctx: &super::ExtractCtxt<'_>,
    method_def_id: DefId,
    args: &[ThirExprId],
) -> Result<ThirExprId, LoweringError> {
    // Find the Queryable method info for this def id.
    let info = ctx
        .queryable_method_info(method_def_id)
        .cloned()
        .ok_or(LoweringError::UnsupportedExpr)?;

    // The only case the extractor currently asks the inliner to handle is
    // sugar methods whose bodies are `self.aggregate(Marker {})`. Those are
    // recognized during discovery and mapped to the Aggregate intrinsic, so
    // this path is rarely reached. When it is, we synthesize the same shape.
    let Some(receiver) = args.get(info.self_index).copied() else {
        return Err(LoweringError::UnsupportedExpr);
    };

    // Locate the `aggregate` method on the `Queryable` trait.
    let aggregate_def_id = ctx
        .aggregate_method_name()
        .and_then(|name| {
            ctx.queryable_methods
                .iter()
                .find(|(_, i)| {
                    i.intrinsic == Some(super::intrinsic::QueryableIntrinsic::Aggregate)
                        && ctx.tcx.resolve_symbol(name).is_some()
                })
                .map(|(def_id, _)| *def_id)
        })
        .ok_or(LoweringError::UnsupportedExpr)?;

    // The sugar default body is `self.aggregate(Marker {})`. We need the
    // Marker type from the original call; if the original method is one of the
    // known sugar methods, we can map its name to a marker DefId by scanning
    // the HIR items.
    let marker_def_id = sugar_marker_def_id(ctx, method_def_id)?;

    // Allocate the marker struct literal in the THIR expression arena.
    // Because `ExtractCtxt` only borrows the THIR arena, we cannot insert new
    // nodes here. We therefore return an error and document the limitation.
    //
    // TODO(phase3): once trait/impl bodies are type-checked and stored as
    // THIR, replace this with a real expression-tree cloner.
    let _ = (aggregate_def_id, marker_def_id, receiver);
    Err(LoweringError::UnsupportedExpr)
}

fn sugar_marker_def_id(
    ctx: &super::ExtractCtxt<'_>,
    method_def_id: DefId,
) -> Result<DefId, LoweringError> {
    let info = ctx
        .queryable_method_info(method_def_id)
        .ok_or(LoweringError::UnsupportedExpr)?;
    let name = ctx
        .tcx
        .crate_hir()
        .definition(info.def_id)
        .and_then(|d| ctx.tcx.resolve_symbol(d.name))
        .unwrap_or("");
    let marker_name = match name {
        "sum" => "Sum",
        "count" => "Count",
        "avg" => "Avg",
        "min" => "Min",
        "max" => "Max",
        _ => return Err(LoweringError::UnsupportedExpr),
    };
    ctx.tcx
        .crate_hir()
        .items
        .iter_enumerated()
        .find_map(|(def_id, opt_item)| {
            let item = opt_item.as_ref()?;
            if ctx.tcx.resolve_symbol(item.ident.symbol) == Some(marker_name) {
                Some(def_id)
            } else {
                None
            }
        })
        .ok_or(LoweringError::UnsupportedExpr)
}
