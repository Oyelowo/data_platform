//! Side-effect-free scalar expressions used inside QIR operators.
//!
//! Expressions are arena-allocated and referenced by `QExprId`. Operators store
//! `QExprId`s, not inline expressions, so that rewrites can share and mutate
//! expressions cheaply.

use yelang_hir::ids::DefId;
use yelang_interner::Symbol;
use yelang_ty::ty::TyId;

pub use crate::ids::{BinderId, QExprId};

/// A scalar expression in QIR.
#[derive(Clone, Debug, PartialEq)]
pub enum QExpr {
    /// A literal value.
    Lit(QLit, TyId),
    /// Reference to a pipeline binder.
    Column(BinderId, TyId),
    /// Field access on a record/object.
    Field(QExprId, Symbol, TyId),
    /// Index into an array/tuple.
    Index(QExprId, QExprId, TyId),
    /// Binary operation.
    Binary(QBinaryOp, QExprId, QExprId, TyId),
    /// Unary operation.
    Unary(QUnaryOp, QExprId, TyId),
    /// Ordinary function call.
    Call(DefId, Vec<QExprId>, TyId),
    /// Method call that did not resolve to a queryable/aggregate plan node.
    MethodCall {
        receiver: QExprId,
        method: DefId,
        args: Vec<QExprId>,
        ty: TyId,
    },
    /// Record/object construction.
    Record(Vec<(Symbol, QExprId)>, TyId),
    /// Tuple construction.
    Tuple(Vec<QExprId>, TyId),
    /// Array construction.
    Array(Vec<QExprId>, TyId),
    /// Conditional expression.
    If(QExprId, QExprId, QExprId, TyId),
    /// Pattern match expression.
    Match {
        scrutinee: QExprId,
        arms: Vec<MatchArm>,
        ty: TyId,
    },
    /// Lambda/closure. Captures are explicit.
    Closure {
        params: Vec<BinderId>,
        body: QExprId,
        captures: Vec<BinderId>,
        ty: TyId,
    },
    /// Let binding.
    Let {
        name: BinderId,
        value: QExprId,
        body: QExprId,
        ty: TyId,
    },
    /// Aggregate call (`q.aggregate(Agg)` or `q.sum()` sugar).
    AggregateCall(AggregateCall, TyId),
    /// Window function call.
    WindowCall {
        input: QExprId,
        func: WindowFunc,
        partition: Vec<QExprId>,
        order: Vec<OrderKey>,
        frame: WindowFrame,
        ty: TyId,
    },
    /// Coerce a value to another type.
    Cast(QExprId, TyId),
    /// A subplan fragment embedded in an expression.
    ///
    /// Produced when a method call on `Queryable`/`Aggregate`/`Iterator` is
    /// lowered to a logical operator. The surrounding expression can consume
    /// it as a collection or scalar value.
    Subplan(crate::ids::LirId, TyId),
    /// Error expression (used when lowering fails partially).
    Error(TyId),
}

impl QExpr {
    /// Return the type of the expression.
    pub fn ty(&self) -> TyId {
        match self {
            QExpr::Lit(_, ty)
            | QExpr::Column(_, ty)
            | QExpr::Field(_, _, ty)
            | QExpr::Index(_, _, ty)
            | QExpr::Binary(_, _, _, ty)
            | QExpr::Unary(_, _, ty)
            | QExpr::Call(_, _, ty)
            | QExpr::MethodCall { ty, .. }
            | QExpr::Record(_, ty)
            | QExpr::Tuple(_, ty)
            | QExpr::Array(_, ty)
            | QExpr::If(_, _, _, ty)
            | QExpr::Match { ty, .. }
            | QExpr::Closure { ty, .. }
            | QExpr::Let { ty, .. }
            | QExpr::AggregateCall(_, ty)
            | QExpr::WindowCall { ty, .. }
            | QExpr::Cast(_, ty)
            | QExpr::Subplan(_, ty)
            | QExpr::Error(ty) => *ty,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MatchArm {
    pub pat: Pattern,
    pub guard: Option<QExprId>,
    pub body: QExprId,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Pattern {
    Wild,
    Literal(QLit),
    Bind(BinderId, TyId),
    Record(Vec<(Symbol, Pattern)>),
    Tuple(Vec<Pattern>),
    Array(Vec<Pattern>),
}

/// Aggregate call embedded in an expression.
#[derive(Clone, Debug, PartialEq)]
pub struct AggregateCall {
    /// DefId of the aggregate config type (e.g., `Sum`, `Avg`, `Percentile`).
    pub agg_def: DefId,
    /// DefId of the `Aggregate` trait impl.
    pub impl_def: DefId,
    /// Classification read from `Aggregate::class()`.
    pub class: AggregateClass,
    /// Input collection expression.
    pub input: QExprId,
    /// Per-row input expression fed to `step`.
    pub per_row: QExprId,
    /// Accumulator type.
    pub acc_ty: TyId,
    /// Output type.
    pub out_ty: TyId,
}

/// Aggregate classification used by the distributed planner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AggregateClass {
    /// Partial states merge directly: sum, count, min, max, bool_and, bool_or.
    Distributive,
    /// Fixed-size intermediate state; merge works: avg, stddev, variance.
    Algebraic,
    /// Must see all values co-located: median, percentile, mode.
    Holistic,
}

/// Window function kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WindowFunc {
    RowNumber,
    Rank,
    DenseRank,
    Enumerate,
    /// User-defined or built-in window function referenced by DefId.
    User(DefId),
}

/// Window frame specification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowFrame {
    Rows(FrameBounds),
    Range(FrameBounds),
    Groups(FrameBounds),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameBounds {
    pub start: FrameBound,
    pub end: FrameBound,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameBound {
    UnboundedPreceding,
    Preceding(u64),
    CurrentRow,
    Following(u64),
    UnboundedFollowing,
}

/// A sort key.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct OrderKey {
    pub expr: QExprId,
    pub dir: Direction,
    pub nulls: NullOrdering,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Direction {
    Asc,
    Desc,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NullOrdering {
    First,
    Last,
}

/// Binary operators.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum QBinaryOp {
    Eq,
    Ne,
    Lt,
    Lte,
    Gt,
    Gte,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    And,
    Or,
    Concat,
    Is,
    As,
}

/// Unary operators.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum QUnaryOp {
    Not,
    Neg,
    BitNot,
}

/// Literal values.
#[derive(Clone, Debug, PartialEq)]
pub enum QLit {
    Int(i128),
    Float(f64),
    Bool(bool),
    Str(Symbol),
    Unit,
}
