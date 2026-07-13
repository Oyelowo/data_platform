/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 31/12/2024
 */

use crate::{Expr, Ident, Type};

#[derive(Debug, Clone, PartialEq)]
pub struct HopRange {
    pub start: Option<Expr>,
    pub end: Option<Expr>,
    pub inclusive: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LinksMatchKind {
    #[default]
    Optional,
    Required,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SelectQ {
    pub projection: Expr,
    pub from: Vec<FromNode>,
    pub links_match_kind: LinksMatchKind,
    pub links: Vec<LinkPath>,
    pub post_links_for: Vec<ForRootModifiers>,
    pub where_clause: Option<Expr>,
    pub group_by: Option<GroupByClause>,
    pub order_by: Option<Vec<OrderByPart>>,
    pub range: Option<Range>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ForRootModifiers {
    pub target: Ident,
    pub modifiers: Modifiers,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Direction {
    /// ->
    Outgoing,
    /// <-
    Incoming,
}

impl std::fmt::Display for Direction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Direction::Outgoing => write!(f, "->"),
            Direction::Incoming => write!(f, "<-"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LinksClause {
    pub match_kind: LinksMatchKind,
    pub paths: Vec<LinkPath>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EdgeDirection {
    Forward,
    Backward,
    Bidirectional,
}

impl std::fmt::Display for EdgeDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EdgeDirection::Forward => write!(f, "->"),
            EdgeDirection::Backward => write!(f, "<-"),
            EdgeDirection::Bidirectional => write!(f, "<->"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OrderByPart {
    pub field: Expr,
    pub direction: SortDirection,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OrderByClause {
    pub orders: Vec<OrderByPart>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Range {
    pub start: Option<Expr>,
    pub end: Option<Expr>,
    pub inclusive: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Modifiers {
    pub filter: Option<Expr>,
    pub order: Option<Vec<OrderByPart>>,
    pub range: Option<Range>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FromNode {
    pub var: Option<Ident>,
    pub bind: Option<Ident>,
    pub ty: Option<Type>,
    pub modifiers: Modifiers,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub var: Option<Ident>,
    pub bind: Option<Ident>,
    pub ty: Option<Type>,
    pub modifiers: Modifiers,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    pub var: Option<Ident>,
    pub bind: Option<Ident>,
    pub ty: Option<Type>,
    pub hops: Option<HopRange>,
    pub modifiers: Modifiers,
    pub direction: EdgeDirection,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LinkSegment {
    pub edge: Edge,
    pub target: Node,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LinkPath {
    pub start: Node,
    pub segments: Vec<LinkSegment>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupByKey {
    pub name: Option<Ident>,
    pub expr: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupByClause {
    pub keys: Vec<GroupByKey>,
    pub into: Ident,
}
