//! Query and selector-chain expressions.
//!
//! These HIR nodes represent `select ... from ... where ...` expressions,
//! mutation queries (`create`, `update`, `upsert`, `delete`, `link`, `unlink`),
//! and binder-bearing array selectors like `users@u[*].id`.

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
    /// `create <var>@<binder>:<Table> { ... } [link ...] [return ...]`
    Create(CreateQuery),
    /// `update <var>@<binder>:<Table> [set ...] [merge ...] [where ...] [return ...]`
    Update(UpdateQuery),
    /// `upsert <var>@<binder>:<Table> { ... } [on conflict ...] [link ...] [return ...]`
    Upsert(UpsertQuery),
    /// `delete <var>@<binder>:<Table> [where ...] [return ...]`
    Delete(DeleteQuery),
    /// `link <path> [, <path> ...] [return ...]`
    Link(LinkQuery),
    /// `unlink <path> [, <path> ...] [return ...]`
    Unlink(UnlinkQuery),
}

/// A `select` query (single- or multi-root).
#[derive(Debug, Clone)]
pub struct SelectQuery {
    pub projection: ExprId,
    pub from: Vec<FromNode>,
    pub links_match_kind: LinksMatchKind,
    pub links: Vec<SelectLinkPath>,
    pub post_links_for: Vec<ForRootModifiers>,
    pub where_clause: Option<ExprId>,
    pub group_by: Option<GroupByClause>,
    pub order_by: Vec<OrderByPart>,
    pub range: Option<QueryRange>,
}

/// Whether `links` traversals are required to match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LinksMatchKind {
    #[default]
    Optional,
    Required,
}

/// A single source in the `from` list.
#[derive(Debug, Clone)]
pub struct FromNode {
    /// The collection expression, e.g. `users`.
    pub source: ExprId,
    /// The source label (e.g. `users` in `from users@u:User`). Used by
    /// `links` paths to anchor traversals from this root.
    pub label: yelang_interner::Symbol,
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

/// `create <var>@<binder>:<Table> { ... }`.
#[derive(Debug, Clone)]
pub struct CreateQuery {
    pub var: yelang_ast::Ident,
    pub binder: PatId,
    pub table: HirTyId,
    pub data: CreateData,
    pub links: Vec<CreateLinkPath>,
    pub return_: Option<ExprId>,
}

/// Payload of a `create`/`upsert` query.
#[derive(Debug, Clone)]
pub enum CreateData {
    /// `create user:User { id: 1, name: 'x' }`
    Object(Vec<(yelang_ast::Ident, ExprId)>),
    /// `create user:User [{ ... }, { ... }]`
    Array(Vec<ExprId>),
}

/// `update <var>@<binder>:<Table> ...`.
#[derive(Debug, Clone)]
pub struct UpdateQuery {
    pub var: yelang_ast::Ident,
    pub binder: PatId,
    pub table: HirTyId,
    pub mutation: UpdateMutation,
    pub links: Vec<CreateLinkPath>,
    pub condition: Option<ExprId>,
    pub return_: Option<ExprId>,
}

/// Payload of an `update` query.
#[derive(Debug, Clone)]
pub enum UpdateMutation {
    /// `update users@u:User { name: 'x' }`
    Merge(Vec<(yelang_ast::Ident, ExprId)>),
    /// `update users@u:User set u.name = 'x'`
    Set(Vec<Setter>),
}

/// A single setter in an `update ... set ...` clause.
#[derive(Debug, Clone)]
pub struct Setter {
    pub path: ExprId,
    pub op: SetterOp,
    pub value: ExprId,
}

/// Operation used in an update setter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetterOp {
    Assign,
    Increment,
    Decrement,
}

/// `upsert <var>@<binder>:<Table> { ... } [on conflict ...]`.
#[derive(Debug, Clone)]
pub struct UpsertQuery {
    pub var: yelang_ast::Ident,
    pub binder: PatId,
    pub table: HirTyId,
    pub data: CreateData,
    pub on_conflict: Option<ConflictClause>,
    pub links: Vec<CreateLinkPath>,
    pub return_: Option<ExprId>,
}

/// `on conflict (fields...) <action>`.
#[derive(Debug, Clone)]
pub struct ConflictClause {
    pub fields: Vec<yelang_ast::Ident>,
    pub action: ConflictAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictAction {
    Replace,
    Merge,
    Ignore,
}

/// `delete <var>@<binder>:<Table> [where ...] [return ...]`.
#[derive(Debug, Clone)]
pub struct DeleteQuery {
    pub var: yelang_ast::Ident,
    pub binder: PatId,
    pub table: HirTyId,
    pub condition: Option<ExprId>,
    pub return_: Option<ExprId>,
}

/// `link <path> [, <path> ...] [return ...]`.
#[derive(Debug, Clone)]
pub struct LinkQuery {
    pub paths: Vec<CreateLinkPath>,
    pub return_: Option<ExprId>,
}

/// `unlink <path> [, <path> ...] [return ...]`.
#[derive(Debug, Clone)]
pub struct UnlinkQuery {
    pub paths: Vec<UnlinkPath>,
    pub return_: Option<ExprId>,
}

/// A path in a `link` statement: `(a)->[edge]->(b)`.
#[derive(Debug, Clone)]
pub struct CreateLinkPath {
    pub segments: Vec<CreatePathSegment>,
}

#[derive(Debug, Clone)]
pub enum CreatePathSegment {
    Node(CreateNode),
    Edge(CreateEdge),
}

/// A node in a `create link` path.
#[derive(Debug, Clone)]
pub struct CreateNode {
    pub var: yelang_ast::Ident,
    pub binder: PatId,
    pub table: HirTyId,
    pub modifiers: NodeModifiers,
}

/// An edge in a `create link` path.
#[derive(Debug, Clone)]
pub struct CreateEdge {
    pub var: yelang_ast::Ident,
    pub binder: PatId,
    pub table: HirTyId,
    pub data: Vec<(yelang_ast::Ident, ExprId)>,
    pub direction: yelang_ast::query::EdgeDirection,
}

/// A path in an `unlink` statement.
#[derive(Debug, Clone)]
pub struct UnlinkPath {
    pub segments: Vec<UnlinkPathSegment>,
}

#[derive(Debug, Clone)]
pub enum UnlinkPathSegment {
    Node(LinkNode),
    Edge(LinkEdge),
}

/// A node referenced in an `unlink` path.
#[derive(Debug, Clone)]
pub struct LinkNode {
    pub var: yelang_ast::Ident,
    pub binder: Option<PatId>,
    pub table: Option<HirTyId>,
    pub modifiers: NodeModifiers,
}

/// An edge referenced in an `unlink` path.
#[derive(Debug, Clone)]
pub struct LinkEdge {
    pub var: yelang_ast::Ident,
    pub binder: Option<PatId>,
    pub table: Option<HirTyId>,
    pub modifiers: NodeModifiers,
    pub direction: yelang_ast::query::EdgeDirection,
}

/// Modifiers allowed on link/unlink path nodes and edges.
#[derive(Debug, Clone, Default)]
pub struct NodeModifiers {
    pub filter: Option<ExprId>,
    pub order_by: Vec<OrderByPart>,
    pub range: Option<QueryRange>,
}

/// A `links` path inside a `select` query.
#[derive(Debug, Clone)]
pub struct SelectLinkPath {
    pub start: SelectLinkNode,
    pub segments: Vec<SelectLinkSegment>,
}

/// One segment of a `links` path: `(upstream)->[edge]->(target)`.
#[derive(Debug, Clone)]
pub struct SelectLinkSegment {
    pub direction: yelang_ast::query::EdgeDirection,
    pub edge: SelectLinkEdge,
    pub target: SelectLinkNode,
}

/// A node inside a `select ... links ...` path.
///
/// The start node is usually a reference to an existing root or intermediate
/// label; segment targets are declarations that introduce a new binder and
/// element type.
#[derive(Debug, Clone)]
pub struct SelectLinkNode {
    pub var: yelang_ast::Ident,
    pub binder: PatId,
    pub elem_ty: Option<HirTyId>,
    pub modifiers: NodeModifiers,
}

/// An edge inside a `select ... links ...` path.
#[derive(Debug, Clone)]
pub struct SelectLinkEdge {
    pub var: yelang_ast::Ident,
    pub binder: PatId,
    pub elem_ty: Option<HirTyId>,
    pub hops: Option<QueryRange>,
    pub modifiers: NodeModifiers,
    pub direction: yelang_ast::query::EdgeDirection,
}

/// A `group by { ... } into <label>` clause.
#[derive(Debug, Clone)]
pub struct GroupByClause {
    pub keys: Vec<GroupByKey>,
    pub into: yelang_ast::Ident,
    pub into_binder: PatId,
}

/// A single key in a `group by { ... }` object.
#[derive(Debug, Clone)]
pub struct GroupByKey {
    pub name: Option<yelang_ast::Ident>,
    pub expr: ExprId,
}

/// Per-root tail modifiers in a multi-root `select`:
/// `for <root> { where ... order by ... range ... }`.
#[derive(Debug, Clone)]
pub struct ForRootModifiers {
    pub target: yelang_ast::Ident,
    pub modifiers: NodeModifiers,
}
