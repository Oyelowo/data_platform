//! [`GroupKey`], [`SortKey`], [`SortSpec`], [`OrderSpec`], [`PlanRange`],
//! [`WindowFunc`], [`WindowKind`], [`WindowFrame`], [`FrameBound`], [`FrameUnit`].

use yelang_interner::Symbol;

use super::{AggKind, ExprRef};

/// A group-by key: either a computed expression or a direct column reference.
///
/// During decorrelation, outer ref columns are added as `Column` keys.
/// No synthetic expression IDs needed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupKey {
    /// A computed expression (e.g. `u.date.year()`).
    Expr(ExprRef),
    /// A direct column reference by name (e.g. outer ref `u.id`).
    Column(Symbol),
}

/// A join key: either a computed expression or a direct column reference.
///
/// Used by `Join` and `GroupJoin` for equi-join conditions.
/// During decorrelation, natural join conditions on outer refs use `Column`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JoinKey {
    /// A computed expression (e.g. `a.id + 1`).
    Expr(ExprRef),
    /// A direct column reference by name (e.g. `u.id`).
    Column(Symbol),
}

/// A sort key: either a computed expression or a direct column reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SortKey {
    /// A computed expression.
    Expr(ExprRef),
    /// A direct column reference by name.
    Column(Symbol),
}

/// A sort specification: a key (expression or column) with a direction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortSpec {
    /// The sort key.
    pub key: SortKey,
    /// `true` for descending.
    pub desc: bool,
}

/// One key in an `ORDER BY` clause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderSpec {
    /// The expression to sort by.
    pub expr: ExprRef,
    /// `true` for descending.
    pub desc: bool,
}

/// A row range (offset/limit or slice).
#[derive(Debug, Clone)]
pub struct PlanRange {
    /// Start offset (inclusive). `None` = 0.
    pub start: Option<ExprRef>,
    /// End bound. `None` = unbounded.
    pub end: Option<ExprRef>,
    /// Whether the end bound is inclusive (`..=`) or exclusive (`..`).
    pub inclusive: bool,
}

// ---------------------------------------------------------------------------
// Window functions
// ---------------------------------------------------------------------------

/// A window function computed over partitions.
///
/// Follows the SQL standard model: `func() OVER (PARTITION BY ... ORDER BY ... frame)`.
/// Used for `ROW_NUMBER`, `RANK`, `LAG`, `LEAD`, and windowed aggregates.
#[derive(Debug, Clone)]
pub struct WindowFunc {
    /// What kind of window function this is.
    pub kind: WindowKind,
    /// PARTITION BY columns.
    pub partition_by: Vec<Symbol>,
    /// ORDER BY within each partition.
    pub order_by: Vec<SortSpec>,
    /// Frame specification (ROWS/RANGE/GROUPS BETWEEN ... AND ...).
    pub frame: Option<WindowFrame>,
    /// Output column name for this window function's result.
    pub output: Symbol,
}

/// The kind of window function.
#[derive(Debug, Clone)]
pub enum WindowKind {
    /// `ROW_NUMBER()` — sequential integer per partition.
    RowNumber,
    /// `RANK()` — rank with gaps for ties.
    Rank,
    /// `DENSE_RANK()` — rank without gaps.
    DenseRank,
    /// `NTILE(n)` — distribute rows into n buckets.
    Ntile { n: ExprRef },
    /// `LAG(expr, offset, default)` — access previous row's value.
    Lag {
        expr: ExprRef,
        offset: ExprRef,
        default: Option<ExprRef>,
    },
    /// `LEAD(expr, offset, default)` — access next row's value.
    Lead {
        expr: ExprRef,
        offset: ExprRef,
        default: Option<ExprRef>,
    },
    /// A windowed aggregate (SUM, COUNT, AVG, etc. over a frame).
    Aggregate(AggKind),
}

/// Window frame specification.
///
/// Defines the subset of rows within a partition that a window function
/// operates on: `ROWS BETWEEN 1 PRECEDING AND CURRENT ROW`.
#[derive(Debug, Clone)]
pub struct WindowFrame {
    /// ROWS, RANGE, or GROUPS.
    pub unit: FrameUnit,
    /// Start bound.
    pub start: FrameBound,
    /// End bound.
    pub end: FrameBound,
}

/// Frame unit: how frame bounds are interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameUnit {
    /// Physical row offsets.
    Rows,
    /// Logical value ranges (ORDER BY key values).
    Range,
    /// Peer group offsets.
    Groups,
}

/// A single frame boundary.
#[derive(Debug, Clone)]
pub enum FrameBound {
    /// `UNBOUNDED PRECEDING`
    UnboundedPreceding,
    /// `<n> PRECEDING`
    Preceding(ExprRef),
    /// `CURRENT ROW`
    CurrentRow,
    /// `<n> FOLLOWING`
    Following(ExprRef),
    /// `UNBOUNDED FOLLOWING`
    UnboundedFollowing,
}
