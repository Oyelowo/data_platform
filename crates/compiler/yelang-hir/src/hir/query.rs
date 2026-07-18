//! Query and selector-chain expressions.
//!
//! These HIR nodes represent `select ... from ... where ...` expressions and
//! binder-bearing array selectors like `users@u[*].id` or `users@u[**].name`.

use crate::ids::{ExprId, HirTyId, PatId};

/// A query expression.
#[derive(Debug, Clone)]
pub struct Query {
    pub kind: QueryKind,
}

/// Kinds of query expression.
#[derive(Debug, Clone)]
pub enum QueryKind {
    /// `select <projection> from ... [where ...] [order by ...] [range ...]`
    Select(SelectQuery),
}

/// A single-root `select` query.
#[derive(Debug, Clone)]
pub struct SelectQuery {
    pub projection: ExprId,
    pub from: Vec<FromNode>,
    pub where_clause: Option<ExprId>,
    pub order_by: Vec<OrderByPart>,
    pub range: Option<QueryRange>,
}

/// A single source in the `from` list.
#[derive(Debug, Clone)]
pub struct FromNode {
    /// The collection expression, e.g. `users`.
    pub source: ExprId,
    /// Binding for the current element, e.g. `u`.
    pub binder: PatId,
    /// Optional `: User` element type annotation.
    pub elem_ty: Option<HirTyId>,
    /// Optional `from (users@u where ...)` filter.
    pub filter: Option<ExprId>,
    /// Optional `order by` modifier.
    pub order_by: Vec<OrderByPart>,
    /// Optional `range` modifier.
    pub range: Option<QueryRange>,
}

/// One `order by <expr> [asc|desc]` part.
#[derive(Debug, Clone)]
pub struct OrderByPart {
    pub expr: ExprId,
    pub direction: yelang_ast::query::SortDirection,
}

/// A `range start..end` (or `..end`, `start..`) clause.
#[derive(Debug, Clone)]
pub struct QueryRange {
    pub start: Option<ExprId>,
    pub end: Option<ExprId>,
    pub inclusive: bool,
}


