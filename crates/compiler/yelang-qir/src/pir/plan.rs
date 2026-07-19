//! Physical QIR plan.

use crate::expr::QExpr;
use crate::ids::{PirArena, PirId, QExprArena};
use crate::pir::operator::PirOp;
use crate::pir::props::{Cost, PhysicalProps};

/// A physical QIR plan.
#[derive(Debug, Default)]
pub struct PhysicalPlan {
    pub operators: PirArena<PirOp>,
    pub props: PirArena<PhysicalProps>,
    pub costs: PirArena<Cost>,
    pub exprs: QExprArena<QExpr>,
    pub root: Option<PirId>,
}

impl PhysicalPlan {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn expr(&self, id: crate::ids::QExprId) -> &QExpr {
        &self.exprs[id]
    }

    pub fn alloc(&mut self, op: PirOp, props: PhysicalProps, cost: Cost) -> PirId {
        let id = self.operators.push(op);
        self.props.push(props);
        self.costs.push(cost);
        id
    }

    pub fn set_root(&mut self, id: PirId) {
        self.root = Some(id);
    }

    pub fn operator(&self, id: PirId) -> &PirOp {
        &self.operators[id]
    }

    pub fn operator_mut(&mut self, id: PirId) -> &mut PirOp {
        &mut self.operators[id]
    }
}
