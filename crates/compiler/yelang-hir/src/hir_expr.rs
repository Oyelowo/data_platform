//! Expressions in HIR.

use yelang_ast::{Ident, Label};

use crate::hir::{Arm, Block, CaptureClause, FieldExpr, Lit};
use crate::hir_body::Param;
use crate::ids::{BodyId, ExprId, PatId, TyId};
use crate::res::Res;

/// Kinds of expressions.
/// All syntax sugar (`for`, `while`, `?`, `async`) has been desugared.
#[derive(Debug, Clone)]
pub enum Expr {
    /// Literal.
    Lit { lit: Lit },
    /// Resolved path.
    Path { res: Res },
    /// Binary operator.
    Binary {
        op: yelang_ast::BinaryOp,
        left: ExprId,
        right: ExprId,
    },
    /// Unary operator.
    Unary {
        op: yelang_ast::UnaryOp,
        expr: ExprId,
    },
    /// Function call.
    Call { func: ExprId, args: Vec<ExprId> },
    /// Method call.
    MethodCall {
        receiver: ExprId,
        method: Ident,
        args: Vec<ExprId>,
        trait_def_id: Option<crate::ids::DefId>,
    },
    /// Field access.
    Field { expr: ExprId, field: Ident },
    /// Array/slice index.
    Index { expr: ExprId, index: ExprId },
    /// Assignment.
    Assign { left: ExprId, right: ExprId },
    /// Block expression.
    Block { block: Block },
    /// Infinite loop.
    Loop { block: Block, label: Option<Label> },
    /// Break from a loop.
    Break {
        label: Option<Label>,
        expr: Option<ExprId>,
    },
    /// Continue a loop.
    Continue { label: Option<Label> },
    /// Return from a function.
    Return { expr: Option<ExprId> },
    /// Match expression.
    Match { expr: ExprId, arms: Vec<Arm> },
    /// If expression.
    If {
        cond: ExprId,
        then_branch: ExprId,
        else_branch: Option<ExprId>,
    },
    /// Closure expression.
    Closure {
        params: Vec<Param>,
        body: BodyId,
        capture_clause: CaptureClause,
    },
    /// Struct literal.
    Struct {
        path: Res,
        fields: Vec<FieldExpr>,
        rest: Option<ExprId>,
    },
    /// Tuple literal.
    Tuple { exprs: Vec<ExprId> },
    /// Array literal.
    Array { exprs: Vec<ExprId> },
    /// Type cast.
    Cast { expr: ExprId, ty: TyId },
    /// Let expression (used inside `if let`).
    Let { pat: PatId, expr: ExprId },
    /// Error recovery.
    Err,
}
