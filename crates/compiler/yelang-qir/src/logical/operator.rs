//! Logical operators in QIR (LIR).
//!
//! LIR is backend-agnostic. Operators form a DAG; child operators are referenced
//! by `LirId`. Scalar expressions inside operators are referenced by `QExprId`.

use yelang_hir::ids::DefId;
use yelang_interner::Symbol;
use yelang_ty::ty::TyId;

use crate::expr::{AggregateClass, OrderKey, QExprId};
use crate::ids::LirId;

/// Source of a `Scan` operator.
#[derive(Clone, Debug, PartialEq)]
pub enum ScanSource {
    /// A named table/collection by symbol.
    Named(Symbol),
    /// An in-memory array value produced by a HIR expression.
    Expr(QExprId),
}

/// Kind of record constructed by a `Construct` operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConstructKind {
    /// A plain record/object.
    Record,
    /// A tuple.
    Tuple,
    /// An array.
    Array,
    /// A facet object in a multi-root `select`.
    Facet,
}

/// Join kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum JoinKind {
    Inner,
    Left,
    Right,
    Full,
    Cross,
    Semi,
    Anti,
}

/// Set operation kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SetOpKind {
    Union,
    UnionAll,
    Intersect,
    IntersectAll,
    Except,
    ExceptAll,
}

/// Edge direction for graph traversal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EdgeDirection {
    Out,
    In,
    Both,
}

/// A logical QIR operator.
#[derive(Clone, Debug, PartialEq)]
pub enum LirOp {
    /// Scan a named collection or an in-memory array value.
    Scan {
        source: ScanSource,
        item_ty: TyId,
    },

    /// Local inline collection.
    Values {
        rows: Vec<QExprId>,
        item_ty: TyId,
    },

    /// Filter rows by predicate.
    Filter {
        input: LirId,
        predicate: QExprId,
    },

    /// Map / project each row.
    Map {
        input: LirId,
        projection: QExprId,
    },

    /// Flatten one level.
    FlatMap {
        input: LirId,
        projection: QExprId,
    },

    /// Order a collection. Output is `Seq`.
    OrderBy {
        input: LirId,
        keys: Vec<OrderKey>,
    },

    /// Slice/range a `Seq`.
    Slice {
        input: LirId,
        offset: QExprId,
        limit: Option<QExprId>,
    },

    /// Distinct rows.
    Distinct {
        input: LirId,
        by: Option<Vec<QExprId>>,
    },

    /// Group rows by key. Output rows are `{ key: K, vals: Queryable[T] }`.
    GroupBy {
        input: LirId,
        key: QExprId,
        key_ty: TyId,
        vals_label: Symbol,
    },

    /// Reduce a collection to a scalar using an aggregate.
    Aggregate {
        input: LirId,
        agg: AggregateOp,
    },

    /// Aggregate combined with grouping (produced by normalization).
    AggregateGroupBy {
        input: LirId,
        group_keys: Vec<QExprId>,
        aggregates: Vec<AggregateOp>,
    },

    /// Binary join.
    Join {
        kind: JoinKind,
        left: LirId,
        right: LirId,
        predicate: Option<QExprId>,
    },

    /// Correlated subquery / lateral.
    DependentJoin {
        outer: LirId,
        inner: LirId,
        predicate: Option<QExprId>,
    },

    /// Graph edge expansion.
    EdgeExpand {
        input: LirId,
        edge: DefId,
        direction: EdgeDirection,
        predicate: Option<QExprId>,
    },

    /// Attach a nested field to each upstream element.
    AttachField {
        input: LirId,
        field: Symbol,
        value_plan: LirId,
    },

    /// Combine independent plans into a record/tuple/array result.
    Construct {
        kind: ConstructKind,
        fields: Vec<(Symbol, LirId)>,
    },

    /// Set operation.
    SetOp {
        op: SetOpKind,
        left: LirId,
        right: LirId,
    },

    /// Window function.
    Window {
        input: LirId,
        func: crate::expr::WindowFunc,
        partition: Vec<QExprId>,
        order: Vec<OrderKey>,
        frame: crate::expr::WindowFrame,
    },

    /// Scalar expression embedded in a plan.
    Expr(QExprId),
}

impl LirOp {
    /// Return child operator ids.
    pub fn children(&self) -> Vec<LirId> {
        match self {
            LirOp::Scan { .. } | LirOp::Values { .. } | LirOp::Expr(_) => vec![],
            LirOp::Filter { input, .. }
            | LirOp::Map { input, .. }
            | LirOp::FlatMap { input, .. }
            | LirOp::OrderBy { input, .. }
            | LirOp::Slice { input, .. }
            | LirOp::Distinct { input, .. }
            | LirOp::GroupBy { input, .. }
            | LirOp::Aggregate { input, .. }
            | LirOp::AggregateGroupBy { input, .. }
            | LirOp::EdgeExpand { input, .. }
            | LirOp::AttachField { input, .. }
            | LirOp::Window { input, .. } => vec![*input],
            LirOp::Join { left, right, .. }
            | LirOp::DependentJoin { outer: left, inner: right, .. }
            | LirOp::SetOp { left, right, .. } => vec![*left, *right],
            LirOp::Construct { fields, .. } => fields.iter().map(|(_, id)| *id).collect(),
        }
    }

    /// Map child ids using a function.
    pub fn map_children<F>(&mut self, mut f: F)
    where
        F: FnMut(LirId) -> LirId,
    {
        match self {
            LirOp::Scan { .. } | LirOp::Values { .. } | LirOp::Expr(_) => {}
            LirOp::Filter { input, .. }
            | LirOp::Map { input, .. }
            | LirOp::FlatMap { input, .. }
            | LirOp::OrderBy { input, .. }
            | LirOp::Slice { input, .. }
            | LirOp::Distinct { input, .. }
            | LirOp::GroupBy { input, .. }
            | LirOp::Aggregate { input, .. }
            | LirOp::AggregateGroupBy { input, .. }
            | LirOp::EdgeExpand { input, .. }
            | LirOp::AttachField { input, .. }
            | LirOp::Window { input, .. } => *input = f(*input),
            LirOp::Join { left, right, .. }
            | LirOp::DependentJoin { outer: left, inner: right, .. }
            | LirOp::SetOp { left, right, .. } => {
                *left = f(*left);
                *right = f(*right);
            }
            LirOp::Construct { fields, .. } => {
                for (_, id) in fields {
                    *id = f(*id);
                }
            }
        }
    }
}

/// An aggregate operation.
#[derive(Clone, Debug, PartialEq)]
pub struct AggregateOp {
    /// DefId of the aggregate config type (e.g., `Sum`, `Avg`, `Percentile`).
    pub agg_def: DefId,
    /// DefId of the `Aggregate` trait impl.
    pub impl_def: DefId,
    /// Classification.
    pub class: AggregateClass,
    /// Per-row input expression fed to `step`.
    pub per_row: QExprId,
    /// Accumulator type.
    pub acc_ty: TyId,
    /// Output type.
    pub out_ty: TyId,
}
