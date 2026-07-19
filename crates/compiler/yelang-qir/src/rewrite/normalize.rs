//! Normalization rewrite batch.
//!
//! Performs simple structural normalizations:
//! - Ensure the plan has a root node.
//! - Elide identity `Map` operators (closure whose body is its parameter).
//! - Elide `Filter` operators whose predicate is the literal `true`.
//!
//! These are intentionally conservative: they only remove operators that are
//! guaranteed no-ops, so the rewrite cannot introduce incorrectness.

use yelang_arena::FxHashMap;

use crate::errors::LoweringError;
use crate::expr::{QExpr, QLit};
use crate::ids::LirId;
use crate::logical::operator::LirOp;
use crate::logical::plan::LogicalPlan;
use crate::logical::props::LogicalProps;
use crate::rewrite::pass::RewritePass;
use crate::rewrite::apply_id_rewrites;

pub struct NormalizePass;

impl RewritePass for NormalizePass {
    fn run(&self, plan: &mut LogicalPlan) -> Result<bool, LoweringError> {
        let mut changed = false;

        // Ensure a root exists.  Other passes rely on plan.root being Some.
        if plan.root.is_none() {
            let ty = plan
                .exprs
                .get(crate::ids::QExprId(0))
                .map(|e| e.ty())
                .unwrap_or_else(|| yelang_ty::ty::TyId::new(1));
            let expr = plan.alloc_expr(crate::expr::QExpr::Error(ty));
            let props = LogicalProps::new(ty);
            let id = plan.alloc_operator(LirOp::Expr(expr), props);
            plan.set_root(id);
            changed = true;
        }

        let ids: Vec<LirId> = crate::rewrite::reachable_ids(plan);
        let mut rewrites: FxHashMap<LirId, LirId> = FxHashMap::default();

        for id in ids {
            match plan.operator(id) {
                LirOp::Map { input, projection } => {
                    if is_identity_closure(plan, *projection) {
                        rewrites.insert(id, *input);
                    }
                }
                LirOp::Filter { input, predicate } => {
                    if is_true_lit(plan, *predicate) {
                        rewrites.insert(id, *input);
                    }
                }
                _ => {}
            }
        }

        if !rewrites.is_empty() {
            apply_id_rewrites(plan, &rewrites);
            changed = true;
        }

        Ok(changed)
    }
}

fn is_identity_closure(plan: &LogicalPlan, expr: crate::ids::QExprId) -> bool {
    match plan.expr(expr) {
        QExpr::Closure { params, body, .. } if params.len() == 1 => {
            matches!(plan.expr(*body), QExpr::Column(b, _) if *b == params[0])
        }
        _ => false,
    }
}

fn is_true_lit(plan: &LogicalPlan, expr: crate::ids::QExprId) -> bool {
    matches!(plan.expr(expr), QExpr::Lit(QLit::Bool(true), _))
}
