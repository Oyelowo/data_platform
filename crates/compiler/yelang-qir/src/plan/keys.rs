//! [`GroupKey`], [`SortKey`], [`SortSpec`], [`OrderSpec`], [`PlanRange`].

use yelang_interner::Symbol;

use super::ExprRef;

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
