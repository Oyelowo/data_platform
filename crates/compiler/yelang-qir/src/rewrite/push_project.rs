//! Projection pushdown: move `Map` projections below operators that do not
//! depend on row contents.
//!
//! Currently supported:
//! - `Map(Slice(input, off, lim), f)` -> `Slice(Map(input, f), off, lim)`
//!
//! This is safe because `Slice` only cares about row counts and ordering, not
//! the row values themselves.

use yelang_arena::FxHashMap;

use crate::errors::LoweringError;
use crate::ids::LirId;
use crate::lir::operator::LirOp;
use crate::lir::plan::LogicalPlan;
use crate::rewrite::pass::RewritePass;
use crate::rewrite::{apply_id_rewrites, reachable_ids};

pub struct PushProjectPass;

impl RewritePass for PushProjectPass {
    fn run(&self, plan: &mut LogicalPlan) -> Result<bool, LoweringError> {
        let ids: Vec<LirId> = reachable_ids(plan);
        let mut rewrites: FxHashMap<LirId, LirId> = FxHashMap::default();

        for id in ids {
            let (slice_input, offset, limit, ordered) = match plan.operator(id) {
                LirOp::Slice {
                    input,
                    offset,
                    limit,
                } => {
                    let ordered = plan.props[id].ordered;
                    (*input, *offset, *limit, ordered)
                }
                _ => continue,
            };

            let (grandchild, projection) = match plan.operator(slice_input) {
                LirOp::Map { input, projection } => (*input, *projection),
                _ => continue,
            };

            let map_out_ty = plan.props[slice_input].output_ty;
            let grandchild_ty = plan.props[grandchild].output_ty;
            let new_slice = plan.slice_unchecked(grandchild, offset, limit, grandchild_ty, ordered);
            let new_map = plan.map(new_slice, projection, map_out_ty);

            rewrites.insert(id, new_map);
        }

        apply_id_rewrites(plan, &rewrites);
        Ok(!rewrites.is_empty())
    }
}
