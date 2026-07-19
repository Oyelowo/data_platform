//! Pipeline builder: convert PIR to ExecOp trees.

use crate::errors::ExecError;
use crate::exec::operator::ExecOp;
use crate::exec::plan::ExecPlan;
use crate::ids::ExecId;
use crate::pir::plan::PhysicalPlan;

/// Build an execution plan from a physical plan.
pub fn build_exec(plan: &PhysicalPlan) -> Result<ExecPlan, ExecError> {
    let mut exec = ExecPlan::empty();
    let root = plan.root.ok_or_else(|| ExecError::Pipeline("empty physical plan".to_string()))?;
    let exec_root = lower_pir(plan, root, &mut exec)?;
    exec.set_root(exec_root);
    Ok(exec)
}

fn lower_pir(
    _plan: &PhysicalPlan,
    _id: crate::ids::PirId,
    _exec: &mut ExecPlan,
) -> Result<ExecId, ExecError> {
    // TODO: implement PIR -> Exec lowering.
    Ok(_exec.alloc(ExecOp::Expr(crate::exec::operator::ExprExec { expr: crate::ids::QExprId(0) })))
}
