//! Logical operators in QIR.

use yelang_interner::Symbol;
use yelang_ty::ty::TyId;

use crate::expr::QExpr;
use crate::ids::QirId;

/// Source of a `Scan` operator.
#[derive(Clone, Debug, PartialEq)]
pub enum ScanSource {
    /// A named table/collection.
    Named(Symbol),
    /// An in-memory array value produced by a HIR expression.
    Expr(crate::ids::QExprId),
}

/// Kind of record constructed by a `Construct` operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstructKind {
    /// A facet object in a multi-root `select`.
    Facet,
    /// A plain record/object.
    Record,
    /// A tuple.
    Tuple,
}

/// A sort key for `OrderBy` / `Window`.
#[derive(Clone, Debug, PartialEq)]
pub struct OrderKey {
    pub expr: QExpr,
    pub descending: bool,
}

/// Aggregate kinds recognized by QIR.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AggregateKind {
    Count,
    Sum,
    Min,
    Max,
    Avg,
}

/// Window kinds recognized by QIR.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowKind {
    RowNumber,
    Rank,
    DenseRank,
    Enumerate,
}

/// Set-operation kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetOpKind {
    Union,
    Intersect,
    Except,
}

/// A logical QIR operator.
///
/// Operators form a DAG. Child operator IDs refer to the `LogicalPlan::operators`
/// arena. Scalar expressions inside operators are stored in the plan's
/// `QExprArena` and referenced by `QExprId` where appropriate.
#[derive(Clone, Debug, PartialEq)]
pub enum Operator {
    /// Scan a named collection or an in-memory array value.
    Scan { source: ScanSource, item_ty: TyId },

    /// Filter a collection by a predicate over its element.
    Filter { input: QirId, predicate: QExpr },

    /// Map each element to a new value.
    Map { input: QirId, projection: QExpr },

    /// flat_map one or more levels.
    FlatMap { input: QirId, levels: u32, projection: QExpr },

    /// Order a collection.
    OrderBy { input: QirId, keys: Vec<OrderKey> },

    /// Slice/range a collection.
    Range {
        input: QirId,
        start: Option<QExpr>,
        end: Option<QExpr>,
        inclusive: bool,
    },

    /// Join operators.
    InnerJoin { left: QirId, right: QirId, predicate: QExpr },
    LeftOuterJoin { left: QirId, right: QirId, predicate: QExpr },
    SemiJoin { left: QirId, right: QirId, predicate: QExpr },
    AntiJoin { left: QirId, right: QirId, predicate: QExpr },
    MarkJoin { left: QirId, right: QirId, predicate: QExpr, marker: Symbol },
    CrossJoin { left: QirId, right: QirId },

    /// Dependent join used as an intermediate during decorrelation.
    DependentJoin { outer: QirId, inner: QirId, predicate: QExpr },

    /// Group an input collection by key expressions.
    GroupBy { input: QirId, keys: Vec<(Symbol, QExpr)>, members_label: Symbol },

    /// Reduce a collection to a scalar.
    Aggregate { input: QirId, kind: AggregateKind },

    /// Window functions.
    Window {
        input: QirId,
        kind: WindowKind,
        partition: Vec<QExpr>,
        order: Vec<OrderKey>,
    },

    /// Set operations.
    SetOp { op: SetOpKind, left: QirId, right: QirId },

    /// Distinct rows.
    Distinct { input: QirId, by: Option<Vec<QExpr>> },

    /// Attach a nested field to each upstream element.
    AttachField { input: QirId, field: Symbol, value_plan: QirId },

    /// Combine independent plans into a struct/object result.
    Construct { kind: ConstructKind, fields: Vec<(Symbol, QirId)> },

    /// Return a scalar value produced by a sub-expression.
    Expr(QExpr),
}
