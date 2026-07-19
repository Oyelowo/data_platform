//! Logical QIR: backend-agnostic operators and lowering from typed HIR.

pub mod links;
pub mod lower;
pub mod operator;
pub mod shape;

pub use operator::{ConstructKind, Operator, ScanSource};
pub use shape::{CorrelationMode, NestedShape};

use crate::errors::LoweringError;
use crate::expr::QExpr;
use crate::ids::{QExprArena, QirArena, QirId};

/// A logical QIR plan.
#[derive(Debug, Default)]
pub struct LogicalPlan {
    /// Arena of logical operators.
    pub operators: QirArena<Operator>,
    /// Arena of scalar expressions referenced by operators.
    pub exprs: QExprArena<QExpr>,
    /// Root operator id.
    pub root: Option<QirId>,
}

impl LogicalPlan {
    /// Create an empty plan. Useful as a skeleton before lowering is complete.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Allocate a new operator and return its id.
    pub fn alloc_operator(&mut self, op: Operator) -> QirId {
        self.operators.push(op)
    }

    /// Allocate a new scalar expression and return its id.
    pub fn alloc_expr(&mut self, expr: QExpr) -> crate::ids::QExprId {
        self.exprs.push(expr)
    }

    /// Set the root operator of the plan.
    pub fn set_root(&mut self, id: QirId) {
        self.root = Some(id);
    }

    /// Lower a typed HIR query into this plan.
    pub fn lower_query(
        &mut self,
        tcx: &yelang_tycheck::tcx::TyCtxt,
        body_id: yelang_hir::ids::BodyId,
        query_id: yelang_hir::ids::QueryId,
    ) -> Result<QirId, LoweringError> {
        lower::lower_query(self, tcx, body_id, query_id)
    }
}
