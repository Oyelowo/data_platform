//! THIR query structures.
//!
//! These mirror the HIR query types but use THIR expression IDs (`ThirExprId`)
//! instead of HIR expression IDs (`ExprId`). This makes the THIR self-contained:
//! the QIR lowering reads query structure from THIR, not HIR.
//!
//! The THIR lowering populates these by lowering each HIR query sub-expression
//! (projection, where clause, order by keys, etc.) to THIR.

use yelang_interner::Symbol;

use crate::ids::{ThirExprId, ThirPatId};
use crate::ty::ThirTyId;

/// A lowered `select` query with all sub-expressions in THIR form.
#[derive(Debug, Clone)]
pub struct ThirSelectQuery {
    /// The projection expression (`select <expr>`).
    pub projection: ThirExprId,
    /// The `from` roots.
    pub from: Vec<ThirFromNode>,
    /// The `links` traversal paths.
    pub links: Vec<ThirLinkPath>,
    /// Pipeline `where` clause (post-links filter).
    pub where_clause: Option<ThirExprId>,
    /// `group by` clause.
    pub group_by: Option<ThirGroupBy>,
    /// `order by` clause.
    pub order_by: Vec<ThirOrderByPart>,
    /// `range` clause.
    pub range: Option<ThirRange>,
}

/// A lowered `from` root node.
#[derive(Debug, Clone)]
pub struct ThirFromNode {
    /// The source expression (e.g., `users()` call).
    pub source: ThirExprId,
    /// The collection label (e.g., `users`).
    pub label: Symbol,
    /// The element binder pattern (e.g., `u`).
    pub binder: ThirPatId,
    /// Optional element type annotation (e.g., `:User`).
    pub elem_ty: Option<ThirTyId>,
    /// Optional inline filter (inside `from` parentheses).
    pub filter: Option<ThirExprId>,
    /// Per-root `order by`.
    pub order_by: Vec<ThirOrderByPart>,
    /// Per-root `range`.
    pub range: Option<ThirRange>,
}

/// A lowered `links` traversal path.
#[derive(Debug, Clone)]
pub struct ThirLinkPath {
    /// The anchor node (starting point).
    pub anchor: ThirLinkNode,
    /// The traversal segments.
    pub segments: Vec<ThirLinkSegment>,
}

/// A node in a link path.
#[derive(Debug, Clone)]
pub struct ThirLinkNode {
    /// The node label.
    pub label: Symbol,
    /// The node binder pattern.
    pub binder: ThirPatId,
    /// Optional type annotation.
    pub ty: Option<ThirTyId>,
    /// Optional filter predicate.
    pub filter: Option<ThirExprId>,
}

/// A segment in a link path (edge + target node).
#[derive(Debug, Clone)]
pub struct ThirLinkSegment {
    /// Traversal direction.
    pub direction: ThirDirection,
    /// The edge node.
    pub edge: ThirLinkNode,
    /// The target node.
    pub target: ThirLinkNode,
    /// Optional hop range for variable-length traversals.
    pub hop_range: Option<ThirRange>,
}

/// Traversal direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThirDirection {
    Forward,
    Backward,
    Both,
}

/// A lowered `group by` clause.
#[derive(Debug, Clone)]
pub struct ThirGroupBy {
    /// Grouping keys: (output_name, key_expression).
    pub keys: Vec<(Symbol, ThirExprId)>,
    /// The `into` label for the group collection.
    pub into: Symbol,
}

/// A lowered `order by` part.
#[derive(Debug, Clone)]
pub struct ThirOrderByPart {
    /// The sort key expression.
    pub expr: ThirExprId,
    /// `true` for descending.
    pub desc: bool,
}

/// A lowered `range` clause.
#[derive(Debug, Clone)]
pub struct ThirRange {
    /// Start offset (inclusive).
    pub start: Option<ThirExprId>,
    /// End bound.
    pub end: Option<ThirExprId>,
    /// Whether the end bound is inclusive.
    pub inclusive: bool,
}
