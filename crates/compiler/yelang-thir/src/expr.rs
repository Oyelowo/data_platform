//! THIR expressions.

use yelang_arena::DefId;
use yelang_ast::{AssignOpKind, BinaryOp, Label, Mutability, UnaryOp};
use yelang_hir::hir::core::Lit;
use yelang_hir::res::Res;
use yelang_interner::Symbol;

use crate::ids::{ThirBodyId, ThirExprId, ThirPatId, ThirStmtId};
use crate::ty::ThirTyId;

/// Kinds of THIR expressions.
#[derive(Debug, Clone)]
pub enum ThirExpr {
    /// Literal value.
    Literal(Lit),
    /// Reference to a defined item, constant, function, or `self` parameter.
    Var(DefId),
    /// Reference to a local variable or parameter introduced by a pattern.
    Local(ThirPatId),
    /// Field access: `base.field`.
    Field { base: ThirExprId, field: Symbol },
    /// Function or method call.
    Call { func: ThirExprId, args: Vec<ThirExprId> },
    /// Closure literal.
    Closure { params: Vec<ThirPatId>, body: ThirBodyId },
    /// Block expression.
    Block { stmts: Vec<ThirStmtId>, tail: Option<ThirExprId> },
    /// Binary operator.
    Binary {
        op: BinaryOp,
        left: ThirExprId,
        right: ThirExprId,
    },
    /// Unary operator.
    Unary { op: UnaryOp, expr: ThirExprId },
    /// Assignment: `left = right`.
    Assign { left: ThirExprId, right: ThirExprId },
    /// Compound assignment: `left += right`.
    AssignOp {
        op: AssignOpKind,
        left: ThirExprId,
        right: ThirExprId,
    },
    /// Array/slice index: `base[index]`.
    Index { base: ThirExprId, index: ThirExprId },
    /// Tuple literal.
    Tuple { fields: Vec<ThirExprId> },
    /// Array literal.
    Array { exprs: Vec<ThirExprId> },
    /// Array repeat literal: `[value; count]`.
    ArrayRepeat { value: ThirExprId, count: ThirExprId },
    /// Struct literal.
    Struct {
        path: Res,
        fields: Vec<(Symbol, ThirExprId)>,
        rest: Option<ThirExprId>,
    },
    /// Object/record literal: `{ x: 1, y: 2 }`.
    Object { fields: Vec<(Symbol, ThirExprId)> },
    /// Range expression.
    Range {
        start: Option<ThirExprId>,
        end: Option<ThirExprId>,
        inclusive: bool,
    },
    /// Type cast: `expr as Ty`.
    Cast { expr: ThirExprId, ty: ThirTyId },
    /// Type ascription: `expr: Ty`.
    TypeAscription { expr: ThirExprId, ty: ThirTyId },
    /// `if` expression; branches are bodies so they can introduce bindings.
    If {
        cond: ThirExprId,
        then_branch: ThirBodyId,
        else_branch: Option<ThirBodyId>,
    },
    /// `match` expression.
    Match { scrutinee: ThirExprId, arms: Vec<ThirArm> },
    /// Infinite loop.
    Loop { body: ThirBodyId, label: Option<Label> },
    /// `break` expression.
    Break { label: Option<Label>, expr: Option<ThirExprId> },
    /// `continue` expression.
    Continue { label: Option<Label> },
    /// `return` expression.
    Return { expr: Option<ThirExprId> },
    /// `expr?` try operator.
    Try { expr: ThirExprId },
    /// `expr.await`.
    Await { expr: ThirExprId },
    /// Reference expression: `&expr` or `&mut expr`.
    Ref { mutability: Mutability, expr: ThirExprId },
    /// Dereference: `*expr`.
    Deref { expr: ThirExprId },
    /// `expr is Type` type test.
    IsType { expr: ThirExprId, ty: ThirTyId },
    /// Query syntax (`select ... from ...`) with all sub-expressions
    /// lowered to THIR. The QIR lowering reads this directly.
    Query(Box<crate::query::ThirSelectQuery>),
    /// Compiler-known intrinsic call: `@name(args)`.
    Intrinsic { name: Symbol, args: Vec<ThirExprId> },
    /// Error recovery.
    Err,
}

/// A single arm in a `match` expression.
#[derive(Debug, Clone)]
pub struct ThirArm {
    pub pat: ThirPatId,
    pub guard: Option<ThirExprId>,
    pub body: ThirBodyId,
}
