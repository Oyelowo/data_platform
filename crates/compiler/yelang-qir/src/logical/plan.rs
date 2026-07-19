//! Logical QIR plan: arena-backed operator tree with expression arena.

use yelang_interner::Symbol;
use yelang_ty::ty::TyId;

use crate::errors::LoweringError;
use crate::expr::{OrderKey, QExpr, QExprId, WindowFrame, WindowFunc};
use crate::ids::{LirArena, LirId, QExprArena};
use crate::logical::operator::{AggregateOp, ConstructKind, EdgeDirection, JoinKind, LirOp, ScanSource, SetOpKind};
use crate::logical::props::{Boundedness, CardinalityClass, LogicalProps};
use crate::volatility::Volatility;

/// A logical QIR plan.
#[derive(Debug, Default)]
pub struct LogicalPlan {
    pub operators: LirArena<LirOp>,
    pub props: LirArena<LogicalProps>,
    pub exprs: QExprArena<QExpr>,
    pub root: Option<LirId>,
    /// Monotonic counter for synthetic binder ids used in operator pipelines.
    next_binder: u32,
}

impl LogicalPlan {
    pub fn empty() -> Self {
        Self::default()
    }

    /// Allocate a fresh `BinderId` for use in per-row expressions.
    pub fn fresh_binder(&mut self) -> crate::ids::BinderId {
        let id = crate::ids::BinderId(self.next_binder);
        self.next_binder = self.next_binder.checked_add(1).expect("binder id overflow");
        id
    }

    pub fn alloc_operator(&mut self, op: LirOp, props: LogicalProps) -> LirId {
        let id = self.operators.push(op);
        self.props.push(props);
        id
    }

    pub fn alloc_expr(&mut self, expr: QExpr) -> QExprId {
        self.exprs.push(expr)
    }

    pub fn set_root(&mut self, id: LirId) {
        self.root = Some(id);
    }

    pub fn operator(&self, id: LirId) -> &LirOp {
        &self.operators[id]
    }

    pub fn operator_mut(&mut self, id: LirId) -> &mut LirOp {
        &mut self.operators[id]
    }

    pub fn expr(&self, id: QExprId) -> &QExpr {
        &self.exprs[id]
    }

    pub fn expr_mut(&mut self, id: QExprId) -> &mut QExpr {
        &mut self.exprs[id]
    }

    // --- builder helpers ---

    pub fn scan(&mut self, source: ScanSource, item_ty: TyId) -> LirId {
        let mut props = LogicalProps::new(item_ty);
        props.cardinality = CardinalityClass::Many;
        props.bounded = Boundedness::Bounded;
        self.alloc_operator(LirOp::Scan { source, item_ty }, props)
    }

    pub fn values(&mut self, rows: Vec<QExprId>, item_ty: TyId) -> LirId {
        let mut props = LogicalProps::new(item_ty);
        props.cardinality = if rows.is_empty() { CardinalityClass::Zero } else { CardinalityClass::Many };
        self.alloc_operator(LirOp::Values { rows, item_ty }, props)
    }

    pub fn filter(&mut self, input: LirId, predicate: QExprId, out_ty: TyId) -> LirId {
        let mut props = LogicalProps::new(out_ty);
        // inherit properties from input
        if let Some(in_props) = self.props.get(input) {
            props.ordered = in_props.ordered;
            props.bounded = in_props.bounded;
            props.volatility = in_props.volatility;
        }
        self.alloc_operator(LirOp::Filter { input, predicate }, props)
    }

    pub fn map(&mut self, input: LirId, projection: QExprId, out_ty: TyId) -> LirId {
        let mut props = LogicalProps::new(out_ty);
        if let Some(in_props) = self.props.get(input) {
            props.ordered = in_props.ordered;
            props.bounded = in_props.bounded;
            props.volatility = in_props.volatility;
        }
        self.alloc_operator(LirOp::Map { input, projection }, props)
    }

    pub fn flat_map(&mut self, input: LirId, projection: QExprId, out_ty: TyId) -> LirId {
        let mut props = LogicalProps::new(out_ty);
        if let Some(in_props) = self.props.get(input) {
            props.bounded = in_props.bounded;
            props.volatility = in_props.volatility;
        }
        props.ordered = false;
        self.alloc_operator(LirOp::FlatMap { input, projection }, props)
    }

    pub fn order_by(&mut self, input: LirId, keys: Vec<OrderKey>, out_ty: TyId) -> LirId {
        let mut props = LogicalProps::new(out_ty);
        props.ordered = true;
        if let Some(in_props) = self.props.get(input) {
            props.bounded = in_props.bounded;
            props.volatility = in_props.volatility;
            props.output_binder = in_props.output_binder;
        }
        self.alloc_operator(LirOp::OrderBy { input, keys }, props)
    }

    pub fn slice(
        &mut self,
        input: LirId,
        offset: QExprId,
        limit: Option<QExprId>,
        out_ty: TyId,
    ) -> Result<LirId, LoweringError> {
        if let Some(in_props) = self.props.get(input) {
            if !in_props.ordered {
                return Err(LoweringError::SliceOnUnordered);
            }
        }
        Ok(self.slice_unchecked(input, offset, limit, out_ty, true))
    }

    /// Slice without requiring an ordered input. Used for `take`/`skip` on
    /// general queryable pipelines where deterministic ordering is not assumed.
    pub fn slice_unordered(
        &mut self,
        input: LirId,
        offset: QExprId,
        limit: Option<QExprId>,
        out_ty: TyId,
    ) -> LirId {
        self.slice_unchecked(input, offset, limit, out_ty, false)
    }

    pub(crate) fn slice_unchecked(
        &mut self,
        input: LirId,
        offset: QExprId,
        limit: Option<QExprId>,
        out_ty: TyId,
        ordered: bool,
    ) -> LirId {
        let mut props = LogicalProps::new(out_ty);
        props.ordered = ordered;
        if let Some(in_props) = self.props.get(input) {
            props.bounded = in_props.bounded;
            props.volatility = in_props.volatility;
            props.output_binder = in_props.output_binder;
        }
        self.alloc_operator(LirOp::Slice { input, offset, limit }, props)
    }

    pub fn distinct(&mut self, input: LirId, by: Option<Vec<QExprId>>, out_ty: TyId) -> LirId {
        let mut props = LogicalProps::new(out_ty);
        if let Some(in_props) = self.props.get(input) {
            props.bounded = in_props.bounded;
            props.volatility = in_props.volatility;
        }
        props.ordered = false;
        self.alloc_operator(LirOp::Distinct { input, by }, props)
    }

    pub fn group_by(
        &mut self,
        input: LirId,
        key: QExprId,
        key_ty: TyId,
        vals_label: Symbol,
        out_ty: TyId,
    ) -> LirId {
        let mut props = LogicalProps::new(out_ty);
        if let Some(in_props) = self.props.get(input) {
            props.bounded = in_props.bounded;
            props.volatility = in_props.volatility;
        }
        props.ordered = false;
        self.alloc_operator(LirOp::GroupBy { input, key, key_ty, vals_label }, props)
    }

    pub fn aggregate(&mut self, input: LirId, agg: AggregateOp, out_ty: TyId) -> LirId {
        let mut props = LogicalProps::new(out_ty);
        if let Some(in_props) = self.props.get(input) {
            props.bounded = in_props.bounded;
            props.volatility = in_props.volatility;
        }
        props.cardinality = CardinalityClass::One;
        self.alloc_operator(LirOp::Aggregate { input, agg }, props)
    }

    pub fn aggregate_group_by(
        &mut self,
        input: LirId,
        group_keys: Vec<QExprId>,
        aggregates: Vec<AggregateOp>,
        out_ty: TyId,
    ) -> LirId {
        let mut props = LogicalProps::new(out_ty);
        if let Some(in_props) = self.props.get(input) {
            props.bounded = in_props.bounded;
            props.volatility = in_props.volatility;
        }
        props.ordered = false;
        props.cardinality = CardinalityClass::Many;
        self.alloc_operator(LirOp::AggregateGroupBy { input, group_keys, aggregates }, props)
    }

    pub fn join(
        &mut self,
        kind: JoinKind,
        left: LirId,
        right: LirId,
        predicate: Option<QExprId>,
        out_ty: TyId,
    ) -> LirId {
        let mut props = LogicalProps::new(out_ty);
        if let (Some(l), Some(r)) = (self.props.get(left), self.props.get(right)) {
            props.bounded = if l.bounded == Boundedness::Bounded && r.bounded == Boundedness::Bounded {
                Boundedness::Bounded
            } else {
                Boundedness::Unbounded
            };
            props.volatility = Volatility::combine(l.volatility, r.volatility);
            props.ordered = kind == JoinKind::Cross && l.ordered;
        }
        self.alloc_operator(LirOp::Join { kind, left, right, predicate }, props)
    }

    pub fn dependent_join(
        &mut self,
        outer: LirId,
        inner: LirId,
        predicate: Option<QExprId>,
        out_ty: TyId,
    ) -> LirId {
        let mut props = LogicalProps::new(out_ty);
        if let (Some(l), Some(r)) = (self.props.get(outer), self.props.get(inner)) {
            props.bounded = if l.bounded == Boundedness::Bounded && r.bounded == Boundedness::Bounded {
                Boundedness::Bounded
            } else {
                Boundedness::Unbounded
            };
            props.volatility = Volatility::combine(l.volatility, r.volatility);
        }
        self.alloc_operator(LirOp::DependentJoin { outer, inner, predicate }, props)
    }

    pub fn edge_expand(
        &mut self,
        input: LirId,
        edge: yelang_hir::ids::DefId,
        direction: EdgeDirection,
        predicate: Option<QExprId>,
        out_ty: TyId,
    ) -> LirId {
        let mut props = LogicalProps::new(out_ty);
        if let Some(in_props) = self.props.get(input) {
            props.bounded = in_props.bounded;
            props.volatility = in_props.volatility;
        }
        props.ordered = false;
        self.alloc_operator(LirOp::EdgeExpand { input, edge, direction, predicate }, props)
    }

    pub fn attach_field(
        &mut self,
        input: LirId,
        field: Symbol,
        value_plan: LirId,
        out_ty: TyId,
    ) -> LirId {
        let mut props = LogicalProps::new(out_ty);
        if let (Some(l), Some(r)) = (self.props.get(input), self.props.get(value_plan)) {
            props.bounded = if l.bounded == Boundedness::Bounded && r.bounded == Boundedness::Bounded {
                Boundedness::Bounded
            } else {
                Boundedness::Unbounded
            };
            props.volatility = Volatility::combine(l.volatility, r.volatility);
            props.ordered = l.ordered && r.ordered;
        }
        self.alloc_operator(LirOp::AttachField { input, field, value_plan }, props)
    }

    pub fn construct(
        &mut self,
        kind: ConstructKind,
        fields: Vec<(Symbol, LirId)>,
        out_ty: TyId,
    ) -> LirId {
        let mut props = LogicalProps::new(out_ty);
        let mut bounded = true;
        let mut volatility = Volatility::Pure;
        let mut ordered = true;
        for (_, id) in &fields {
            if let Some(p) = self.props.get(*id) {
                if p.bounded == Boundedness::Unbounded {
                    bounded = false;
                }
                volatility = Volatility::combine(volatility, p.volatility);
                ordered &= p.ordered;
            }
        }
        props.bounded = if bounded { Boundedness::Bounded } else { Boundedness::Unbounded };
        props.volatility = volatility;
        props.ordered = ordered;
        self.alloc_operator(LirOp::Construct { kind, fields }, props)
    }

    pub fn set_op(&mut self, op: SetOpKind, left: LirId, right: LirId, out_ty: TyId) -> LirId {
        let mut props = LogicalProps::new(out_ty);
        if let (Some(l), Some(r)) = (self.props.get(left), self.props.get(right)) {
            props.bounded = if l.bounded == Boundedness::Bounded && r.bounded == Boundedness::Bounded {
                Boundedness::Bounded
            } else {
                Boundedness::Unbounded
            };
            props.volatility = Volatility::combine(l.volatility, r.volatility);
        }
        props.ordered = false;
        self.alloc_operator(LirOp::SetOp { op, left, right }, props)
    }

    pub fn window(
        &mut self,
        input: LirId,
        func: WindowFunc,
        partition: Vec<QExprId>,
        order: Vec<OrderKey>,
        frame: WindowFrame,
        out_ty: TyId,
    ) -> LirId {
        let mut props = LogicalProps::new(out_ty);
        if let Some(in_props) = self.props.get(input) {
            props.bounded = in_props.bounded;
            props.volatility = in_props.volatility;
        }
        self.alloc_operator(LirOp::Window { input, func, partition, order, frame }, props)
    }

    pub fn expr_op(&mut self, expr: QExprId, out_ty: TyId) -> LirId {
        let props = LogicalProps::new(out_ty);
        self.alloc_operator(LirOp::Expr(expr), props)
    }
}


