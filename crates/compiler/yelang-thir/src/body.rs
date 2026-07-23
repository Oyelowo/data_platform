//! THIR bodies.
//!
//! A `ThirBody` is a self-contained unit with parameters and a value
//! expression. `ThirBodies` owns all bodies produced during HIR → THIR
//! lowering.

use slotmap::SlotMap;
use yelang_arena::FxHashMap;
use yelang_hir::ids::{ExprId, QueryId};
use yelang_interner::Symbol;

use crate::expr::ThirExpr;
use crate::ids::{ThirBodyId, ThirExprId, ThirPatId};

#[derive(Debug, Clone)]
pub struct ThirBody {
    pub params: Vec<ThirPatId>,
    pub value: ThirExprId,
}

/// Lowered THIR sub-expressions for a query.
///
/// When the THIR lowering encounters `Expr::Query(query_id)`, it also
/// lowers the query's sub-expressions (projection, where, order by, etc.)
/// and stores them here. The plan extraction then uses these THIR expr
/// IDs instead of HIR expr IDs.
#[derive(Debug, Clone)]
pub struct QueryLowering {
    /// The projection expression (`select <expr>`).
    pub projection: ThirExprId,
    /// Pipeline `where` clause (post-links filter).
    pub where_clause: Option<ThirExprId>,
    /// `order by` key expressions.
    pub order_by: Vec<ThirExprId>,
    /// `group by` key expressions: (output_name, key_expr).
    pub group_by_keys: Vec<(Symbol, ThirExprId)>,
    /// `from` source expressions.
    pub from_sources: Vec<ThirExprId>,
    /// Per-root `where` filters (inside `from` node parentheses).
    pub from_filters: Vec<Option<ThirExprId>>,
    /// `range` start/end expressions.
    pub range_start: Option<ThirExprId>,
    pub range_end: Option<ThirExprId>,
}

#[derive(Debug, Clone, Default)]
pub struct ThirBodies {
    pub bodies: SlotMap<ThirBodyId, ThirBody>,
    /// All THIR expressions produced during lowering.
    /// The QIR analysis walks these directly — no HIR dependency.
    pub exprs: SlotMap<ThirExprId, ThirExpr>,
    /// Lowered query sub-expressions, keyed by HIR QueryId.
    pub query_lowerings: FxHashMap<QueryId, QueryLowering>,
    /// HIR ExprId → THIR ThirExprId.
    pub expr_mapping: FxHashMap<ExprId, ThirExprId>,
}

impl ThirBodies {
    pub fn alloc(&mut self, params: Vec<ThirPatId>, value: ThirExprId) -> ThirBodyId {
        self.bodies.insert(ThirBody { params, value })
    }

    /// Look up the lowered sub-expressions for a query.
    pub fn query_lowering(&self, query_id: QueryId) -> Option<&QueryLowering> {
        self.query_lowerings.get(&query_id)
    }
}
