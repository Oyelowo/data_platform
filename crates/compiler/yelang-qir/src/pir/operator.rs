//! Physical operators in QIR (PIR).

use yelang_hir::ids::DefId;
use yelang_interner::Symbol;
use yelang_ty::ty::TyId;

use crate::expr::{OrderKey, QExprId, WindowFrame, WindowFunc};
use crate::ids::PirId;
pub use crate::logical::operator::JoinKind;
use crate::logical::operator::{ConstructKind, EdgeDirection};
use crate::logical::operator::ScanSource;

/// Kind of data exchange between physical operators.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ExchangeKind {
    /// Keep data on the current worker (single-node identity exchange).
    Single,
    /// Broadcast the entire input to all workers.
    Broadcast,
    /// Repartition by a hash of the given expressions.
    RepartitionBy(Vec<QExprId>),
    /// Repartition by sorted ranges.
    RangePartition(Vec<OrderKey>),
    /// Gather all partitions to a single worker.
    Gather,
}

/// A physical QIR operator.
#[derive(Clone, Debug, PartialEq)]
pub enum PirOp {
    /// Scan a named collection or inline values.
    TableScan {
        source: ScanSource,
        predicate: Option<QExprId>,
        projection: crate::demand::DemandSet,
    },
    Values {
        rows: Vec<QExprId>,
    },

    /// Row-level operators.
    Filter {
        input: PirId,
        predicate: QExprId,
    },
    Project {
        input: PirId,
        projection: QExprId,
    },
    FlatMap {
        input: PirId,
        projection: QExprId,
    },

    /// Sorting / slicing.
    Sort {
        input: PirId,
        keys: Vec<OrderKey>,
    },
    TopK {
        input: PirId,
        keys: Vec<OrderKey>,
        k: usize,
    },
    Slice {
        input: PirId,
        offset: usize,
        limit: Option<usize>,
    },

    /// Joins.
    HashJoin {
        build: PirId,
        probe: PirId,
        build_key: QExprId,
        probe_key: QExprId,
        kind: JoinKind,
    },
    MergeJoin {
        left: PirId,
        right: PirId,
        left_keys: Vec<OrderKey>,
        right_keys: Vec<OrderKey>,
        kind: JoinKind,
    },
    NestedLoopJoin {
        outer: PirId,
        inner: PirId,
        predicate: Option<QExprId>,
        kind: JoinKind,
    },

    /// Group rows by a key without applying a reduction aggregate.
    /// The executor collects the matching rows into a nested collection.
    GroupBy {
        input: PirId,
        key: QExprId,
    },

    /// Aggregations.
    HashAggregate {
        input: PirId,
        group_keys: Vec<QExprId>,
        aggregates: Vec<PhysicalAggregateOp>,
        mode: AggMode,
    },
    SortAggregate {
        input: PirId,
        group_keys: Vec<QExprId>,
        aggregates: Vec<PhysicalAggregateOp>,
        mode: AggMode,
    },
    StreamingAggregate {
        input: PirId,
        group_keys: Vec<QExprId>,
        aggregates: Vec<PhysicalAggregateOp>,
    },

    /// Set ops.
    Union {
        inputs: Vec<PirId>,
    },
    UnionAll {
        inputs: Vec<PirId>,
    },
    Intersect {
        left: PirId,
        right: PirId,
    },
    Except {
        left: PirId,
        right: PirId,
    },
    Distinct {
        input: PirId,
        by: Option<Vec<QExprId>>,
    },

    /// Exchange / distribution enforcers.
    Exchange {
        input: PirId,
        kind: ExchangeKind,
    },
    LocalRepartition {
        input: PirId,
        kind: RepartitionKind,
    },

    /// Graph traversal.
    EdgeExpand {
        input: PirId,
        edge: DefId,
        direction: EdgeDirection,
        predicate: Option<QExprId>,
    },

    /// Construction / materialization.
    Construct {
        kind: ConstructKind,
        fields: Vec<(Symbol, PirId)>,
    },
    AttachField {
        input: PirId,
        field: Symbol,
        value_plan: PirId,
    },

    /// Window.
    Window {
        input: PirId,
        func: WindowFunc,
        partition: Vec<QExprId>,
        order: Vec<OrderKey>,
        frame: WindowFrame,
    },

    /// Scalar.
    Expr(QExprId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AggMode {
    Partial,
    Final,
    Full,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PhysicalAggregateOp {
    pub agg_def: DefId,
    pub impl_def: DefId,
    pub class: crate::expr::AggregateClass,
    /// Per-row input expression fed to `step`.
    pub input_expr: QExprId,
    /// Closure `() -> Acc` producing the initial accumulator.
    pub init: QExprId,
    /// Closure `(Acc, In) -> Acc` consuming one row.
    pub step: QExprId,
    /// Closure `(Acc, Acc) -> Acc` merging partial accumulators.
    pub merge: QExprId,
    /// Closure `Acc -> Out` producing the final result.
    pub finish: QExprId,
    /// Aggregate config value (e.g. `Percentile { p: 0.5 }`).
    pub config: QExprId,
    pub acc_ty: TyId,
    pub out_ty: TyId,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum RepartitionKind {
    Hash(Vec<QExprId>),
    Range(Vec<OrderKey>),
}

impl PirOp {
    pub fn children(&self) -> Vec<PirId> {
        match self {
            PirOp::TableScan { .. } | PirOp::Values { .. } | PirOp::Expr(_) => vec![],
            PirOp::Filter { input, .. }
            | PirOp::Project { input, .. }
            | PirOp::FlatMap { input, .. }
            | PirOp::Sort { input, .. }
            | PirOp::TopK { input, .. }
            | PirOp::Slice { input, .. }
            | PirOp::Distinct { input, .. }
            | PirOp::GroupBy { input, .. }
            | PirOp::HashAggregate { input, .. }
            | PirOp::SortAggregate { input, .. }
            | PirOp::StreamingAggregate { input, .. }
            | PirOp::Exchange { input, .. }
            | PirOp::LocalRepartition { input, .. }
            | PirOp::EdgeExpand { input, .. }
            | PirOp::AttachField { input, .. }
            | PirOp::Window { input, .. } => vec![*input],
            PirOp::HashJoin { build, probe, .. }
            | PirOp::MergeJoin { left: build, right: probe, .. }
            | PirOp::NestedLoopJoin { outer: build, inner: probe, .. }
            | PirOp::Intersect { left: build, right: probe }
            | PirOp::Except { left: build, right: probe } => vec![*build, *probe],
            PirOp::Union { inputs } | PirOp::UnionAll { inputs } => inputs.clone(),
            PirOp::Construct { fields, .. } => fields.iter().map(|(_, id)| *id).collect(),
        }
    }

    pub fn map_children<F>(&mut self, mut f: F)
    where
        F: FnMut(PirId) -> PirId,
    {
        match self {
            PirOp::TableScan { .. } | PirOp::Values { .. } | PirOp::Expr(_) => {}
            PirOp::Filter { input, .. }
            | PirOp::Project { input, .. }
            | PirOp::FlatMap { input, .. }
            | PirOp::Sort { input, .. }
            | PirOp::TopK { input, .. }
            | PirOp::Slice { input, .. }
            | PirOp::Distinct { input, .. }
            | PirOp::GroupBy { input, .. }
            | PirOp::HashAggregate { input, .. }
            | PirOp::SortAggregate { input, .. }
            | PirOp::StreamingAggregate { input, .. }
            | PirOp::Exchange { input, .. }
            | PirOp::LocalRepartition { input, .. }
            | PirOp::EdgeExpand { input, .. }
            | PirOp::AttachField { input, .. }
            | PirOp::Window { input, .. } => *input = f(*input),
            PirOp::HashJoin { build, probe, .. }
            | PirOp::MergeJoin { left: build, right: probe, .. }
            | PirOp::NestedLoopJoin { outer: build, inner: probe, .. }
            | PirOp::Intersect { left: build, right: probe }
            | PirOp::Except { left: build, right: probe } => {
                *build = f(*build);
                *probe = f(*probe);
            }
            PirOp::Union { inputs } | PirOp::UnionAll { inputs } => {
                for id in inputs {
                    *id = f(*id);
                }
            }
            PirOp::Construct { fields, .. } => {
                for (_, id) in fields {
                    *id = f(*id);
                }
            }
        }
    }
}
