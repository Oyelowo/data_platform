//! Filter pushdown: move `Filter` predicates below operators where it is
//! semantically safe and cheaper to evaluate.
//!
//! The main supported rewrite is pushing a filter through a `Map`:
//!
//!   Filter(Map(input, x -> e), (y -> p))  =>  Map(Filter(input, x -> p[y := e]), x -> e)
//!
//! When the predicate is a closure, its parameter is replaced by the map body.
//! When the predicate is a plain boolean expression, it is assumed to already be
//! expressed over the map's output row binder and is rewritten by substituting
//! that binder with the map body.

use yelang_arena::FxHashMap;

use crate::errors::LoweringError;
use crate::ids::{BinderId, LirId, QExprId};
use crate::lir::operator::LirOp;
use crate::lir::plan::LogicalPlan;
use crate::rewrite::pass::RewritePass;
use crate::rewrite::{apply_id_rewrites, as_closure, reachable_ids};
use crate::util::subst::subst_columns;

pub struct PushFilterPass;

impl RewritePass for PushFilterPass {
    fn run(&self, plan: &mut LogicalPlan) -> Result<bool, LoweringError> {
        let ids: Vec<LirId> = reachable_ids(plan);
        let mut rewrites: FxHashMap<LirId, LirId> = FxHashMap::default();

        for id in ids {
            let (map_id, predicate) = match plan.operator(id) {
                LirOp::Filter {
                    input: map_id,
                    predicate,
                } => (*map_id, *predicate),
                _ => continue,
            };

            let (original_id, projection) = match plan.operator(map_id) {
                LirOp::Map {
                    input: original_id,
                    projection,
                } => (*original_id, *projection),
                _ => continue,
            };

            let Some((map_binder, map_body)) = as_closure(plan, projection) else {
                continue;
            };

            let input_ty = plan.props[original_id].output_ty;
            let new_predicate = build_pushed_predicate(plan, predicate, map_binder, map_body);
            let new_filter_id = plan.filter(original_id, new_predicate, input_ty);

            let out_ty = plan.props[map_id].output_ty;
            let new_map_id = plan.map(new_filter_id, projection, out_ty);

            rewrites.insert(id, new_map_id);
        }

        apply_id_rewrites(plan, &rewrites);
        Ok(!rewrites.is_empty())
    }
}

/// Build a predicate over the map's input binder by substituting the map body
/// for the predicate's row reference.
fn build_pushed_predicate(
    plan: &mut LogicalPlan,
    predicate: QExprId,
    map_binder: BinderId,
    map_body: QExprId,
) -> QExprId {
    let (pred_expr, pred_binders) = if let Some((pred_param, pred_body)) = as_closure(plan, predicate)
    {
        // Predicate is a closure: substitute its parameter with the map body and
        // return the body as a plain boolean expression over the map input binder.
        (pred_body, vec![pred_param])
    } else {
        // Plain expression: substitute the map binder directly.  This path is
        // used when the predicate already references the row binder used by the
        // map projection.
        (predicate, vec![map_binder])
    };

    let mut subst = FxHashMap::default();
    for b in pred_binders {
        subst.insert(b, map_body);
    }
    subst_columns(plan, pred_expr, &subst)
}
