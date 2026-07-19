//! Side-effect-free scalar expressions used inside QIR operators.

use yelang_interner::Symbol;
use yelang_ty::ty::TyId;

/// A scalar expression in QIR.
///
/// These expressions have no local scopes, mutation, or control flow. They
/// operate on values produced by operators and on literals.
#[derive(Clone, Debug, PartialEq)]
pub enum QExpr {
    /// A literal value (int, float, bool, string, etc.).
    Lit(QLit),
    /// Reference to an operator output by index in the parent scope.
    Var { index: u32, ty: TyId },
    /// Field access on a record/object.
    Field { base: Box<QExpr>, field: Symbol, ty: TyId },
    /// Binary operation.
    Binary { op: QBinaryOp, left: Box<QExpr>, right: Box<QExpr>, ty: TyId },
    /// Unary operation.
    Unary { op: QUnaryOp, expr: Box<QExpr>, ty: TyId },
    /// Function or method call.
    Call { callee: Box<QExpr>, args: Vec<QExpr>, ty: TyId },
    /// Record/object construction.
    Record { fields: Vec<(Symbol, QExpr)>, ty: TyId },
    /// Tuple construction.
    Tuple { elems: Vec<QExpr>, ty: TyId },
    /// Conditional expression.
    If { cond: Box<QExpr>, then_branch: Box<QExpr>, else_branch: Box<QExpr>, ty: TyId },
    /// Error expression (used when lowering fails partially).
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QUnaryOp {
    Not,
    Neg,
}

#[derive(Clone, Debug, PartialEq)]
pub enum QLit {
    Int(i128),
    Float(f64),
    Bool(bool),
    Str(Symbol),
    Null,
}
