//! Map fusion: merge adjacent `Map` operators by composing their projections.
//!
//! `Map(f).Map(g)` becomes `Map(x -> g(f(x)))`.  This reduces the number of
//! times each row is materialized and exposes more opportunities to the
//! physical planner (e.g. a single projection kernel).

use yelang_arena::FxHashMap;

use crate::errors::LoweringError;
use crate::expr::QExpr;
use crate::ids::{BinderId, LirId, QExprId};
use crate::lir::operator::LirOp;
use crate::lir::plan::LogicalPlan;
use crate::rewrite::pass::RewritePass;
use crate::rewrite::{apply_id_rewrites, as_closure, reachable_ids};
use crate::util::subst::subst_columns;

pub struct MergeMapsPass;

impl RewritePass for MergeMapsPass {
    fn run(&self, plan: &mut LogicalPlan) -> Result<bool, LoweringError> {
        let ids: Vec<LirId> = reachable_ids(plan);
        let mut rewrites: FxHashMap<LirId, LirId> = FxHashMap::default();

        for id in ids {
            let (inner_id, proj2) = match plan.operator(id) {
                LirOp::Map {
                    input: inner_id,
                    projection: proj2,
                } => (*inner_id, *proj2),
                _ => continue,
            };

            let (original_id, proj1) = match plan.operator(inner_id) {
                LirOp::Map {
                    input: original_id,
                    projection: proj1,
                } => (*original_id, *proj1),
                _ => continue,
            };

            let Some((b2, body2)) = as_closure(plan, proj2) else {
                continue;
            };
            let Some((b1, body1)) = as_closure(plan, proj1) else {
                continue;
            };

            // Compose: x -> g(f(x))  where  f(x)=body1  and  g(y)=body2.
            let mut subst = FxHashMap::default();
            subst.insert(b2, body1);
            let fused_body = subst_columns(plan, body2, &subst);

            let fused_proj = build_closure(plan, b1, fused_body, plan.expr(proj2).ty());
            let out_ty = plan.props[inner_id].output_ty;
            let fused_id = plan.map(original_id, fused_proj, out_ty);

            rewrites.insert(id, fused_id);
        }

        apply_id_rewrites(plan, &rewrites);
        Ok(!rewrites.is_empty())
    }
}

fn build_closure(
    plan: &mut LogicalPlan,
    param: BinderId,
    body: QExprId,
    ty: yelang_ty::ty::TyId,
) -> QExprId {
    use crate::util::subst::free_binders;

    let mut free = free_binders(plan, body);
    free.remove(&param);
    plan.alloc_expr(QExpr::Closure {
        params: vec![param],
        body,
        captures: free.into_iter().collect(),
        ty,
    })
}
