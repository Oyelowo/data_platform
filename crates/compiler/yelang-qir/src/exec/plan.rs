//! Execution plan: arena-backed ExecOp tree.

use crate::ids::{ExecArena, ExecId};
use crate::exec::operator::ExecOp;

/// An execution plan.
#[derive(Debug, Default)]
pub struct ExecPlan {
    pub operators: ExecArena<ExecOp>,
    pub root: Option<ExecId>,
}

impl ExecPlan {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn alloc(&mut self, op: ExecOp) -> ExecId {
        self.operators.push(op)
    }

    pub fn set_root(&mut self, id: ExecId) {
        self.root = Some(id);
    }

    pub fn operator(&self, id: ExecId) -> &ExecOp {
        &self.operators[id]
    }

    pub fn operator_mut(&mut self, id: ExecId) -> &mut ExecOp {
        &mut self.operators[id]
    }
}
