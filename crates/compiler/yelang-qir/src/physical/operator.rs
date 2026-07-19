//! Physical operators in QIR.

use yelang_interner::Symbol;
use crate::expr::QExpr;
use crate::ids::PhysId;
use crate::logical::operator::{AggregateKind, ConstructKind, OrderKey, SetOpKind, WindowKind};
use crate::logical::operator::ScanSource;

/// Kind of data exchange between physical operators.
#[derive(Clone, Debug, PartialEq)]
pub enum ExchangeKind {
    /// Keep data on the current worker (single-node).
    Single,
    /// Broadcast the entire input to all workers.
    Broadcast,
    /// Repartition by a hash of the given expressions.
    RepartitionBy(Vec<QExpr>),
    /// Gather all partitions to a single worker.
    Gather,
}

/// A physical QIR operator.
#[derive(Clone, Debug, PartialEq)]
pub enum PhysOperator {
    TableScan { source: ScanSource, predicate: Option<QExpr> },
    Filter { input: PhysId, predicate: QExpr },
    Map { input: PhysId, projection: QExpr },
    FlatMap { input: PhysId, levels: u32, projection: QExpr },
    Sort { input: PhysId, keys: Vec<OrderKey> },
    Slice { input: PhysId, start: usize, end: Option<usize> },
    HashJoin { build: PhysId, probe: PhysId, build_key: QExpr, probe_key: QExpr },
    MergeJoin { left: PhysId, right: PhysId, keys: Vec<(QExpr, QExpr)> },
    NestedLoopJoin { outer: PhysId, inner: PhysId, predicate: QExpr },
    GroupJoin { outer: PhysId, inner: PhysId, key: (QExpr, QExpr), aggregate: AggregateKind },
    HashGroupBy { input: PhysId, keys: Vec<OrderKey>, members_label: Symbol },
    SortGroupBy { input: PhysId, keys: Vec<OrderKey>, members_label: Symbol },
    Aggregate { input: PhysId, kind: AggregateKind },
    Window { input: PhysId, kind: WindowKind, partition: Vec<QExpr>, order: Vec<OrderKey> },
    SetOp { op: SetOpKind, left: PhysId, right: PhysId },
    Distinct { input: PhysId, by: Option<Vec<QExpr>> },
    AttachField { input: PhysId, field: Symbol, value_plan: PhysId },
    Construct { kind: ConstructKind, fields: Vec<(Symbol, PhysId)> },
    Exchange { input: PhysId, kind: ExchangeKind },
    Gather { inputs: Vec<PhysId> },
    Expr(QExpr),
}
