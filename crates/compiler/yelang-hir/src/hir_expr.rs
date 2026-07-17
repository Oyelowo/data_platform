//! Expressions in HIR.

use yelang_ast::{Ident, Label};
use yelang_lexer::Span;

use crate::hir::{Arm, Block, CaptureClause, FieldExpr, Lit};
use crate::hir_body::Param;
use crate::hir_pat::Pat;
use crate::hir_ty::Ty;
use crate::ids::{BodyId, HirId};
use crate::res::Res;

/// An expression node.
#[derive(Debug, Clone)]
pub struct Expr {
    pub hir_id: HirId,
    pub kind: ExprKind,
    pub span: Span,
    pub ty: Ty,
}

/// Kinds of expressions.  
/// All syntax sugar (`for`, `while`, `?`, `async`) has been desugared.
#[derive(Debug, Clone)]
pub enum ExprKind {
    /// Literal.
    Lit { lit: Lit },
    /// Resolved path.
    Path { res: Res },
    /// Binary operator.
    Binary {
        op: yelang_ast::BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// Unary operator.
    Unary {
        op: yelang_ast::UnaryOp,
        expr: Box<Expr>,
    },
    /// Function call.
    Call { func: Box<Expr>, args: Vec<Expr> },
    /// Method call.
    MethodCall {
        receiver: Box<Expr>,
        method: Ident,
        args: Vec<Expr>,
        trait_def_id: Option<crate::ids::DefId>,
    },
    /// Field access.
    Field { expr: Box<Expr>, field: Ident },
    /// Array/slice index.
    Index { expr: Box<Expr>, index: Box<Expr> },
    /// Assignment.
    Assign { left: Box<Expr>, right: Box<Expr> },
    /// Block expression.
    Block { block: Block },
    /// Infinite loop.
    Loop { block: Block, label: Option<Label> },
    /// Break from a loop.
    Break {
        label: Option<Label>,
        expr: Option<Box<Expr>>,
    },
    /// Continue a loop.
    Continue { label: Option<Label> },
    /// Return from a function.
    Return { expr: Option<Box<Expr>> },
    /// Match expression.
    Match { expr: Box<Expr>, arms: Vec<Arm> },
    /// If expression.
    If {
        cond: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Option<Box<Expr>>,
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
        rest: Option<Box<Expr>>,
    },
    /// Tuple literal.
    Tuple { exprs: Vec<Expr> },
    /// Array literal.
    Array { exprs: Vec<Expr> },
    /// Type cast.
    Cast { expr: Box<Expr>, ty: Ty },
    /// Let expression (used inside `if let`).
    Let { pat: Pat, expr: Box<Expr> },
    /// Error recovery.
    Err,
}
