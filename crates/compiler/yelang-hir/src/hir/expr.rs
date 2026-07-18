//! Expressions in HIR.

use yelang_ast::{AssignOpKind, Ident, Label};

use crate::hir::core::{Arm, Block, CaptureClause, FieldExpr, Lit};
use crate::hir::body::Param;
use crate::ids::{BodyId, ExprId, PatId, SyntaxTyId};
use crate::res::Res;

/// Kind of generator expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeneratorKind {
    /// `gen { }` — a sync generator.
    Gen,
    /// `gen async { }` — an async generator.
    AsyncGen,
}

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
    Cast { expr: ExprId, ty: SyntaxTyId },
    /// Let expression (used inside `if let`).
    Let { pat: PatId, expr: ExprId },
    /// Compound assignment: `a += b`.
    AssignOp { op: AssignOpKind, left: ExprId, right: ExprId },
    /// Destructuring assignment: `(a, b) = value`.
    DestructureAssign { pat: PatId, value: ExprId },
    /// Range expression: `1..10`, `1..=10`, `..`, `..5`, `5..`.
    Range { start: Option<ExprId>, end: Option<ExprId>, inclusive: bool },
    /// Object literal: `{ x: 1, y: 2 }`.
    Object { fields: Vec<FieldExpr> },
    /// `expr is Type` type test.
    IsType { expr: ExprId, ty: SyntaxTyId },
    /// `expr?` — try operator (desugared, but kept as a node for clarity).
    Try { expr: ExprId },
    /// `expr.await`.
    Await { expr: ExprId },
    /// `async { ... }` block.
    Async { body: BodyId },
    /// `gen { ... }` or `gen async { ... }`.
    Gen { kind: GeneratorKind, body: BodyId },
    /// Type ascription: `expr: Type`.
    TypeAscription { expr: ExprId, ty: SyntaxTyId },
    /// Document/JSON access: `doc.name` or `doc["name"]`.
    DocumentAccess { base: ExprId, projection: Vec<DocumentProjection> },
    /// List/set/dict comprehension.
    Comprehension {
        kind: ComprehensionKind,
        element: ExprId,
        variables: Vec<(PatId, ExprId)>,
        condition: Option<ExprId>,
    },
    /// Error recovery.
    Err,
}

/// A single projection step in a document access (`doc.{ ... }`).
#[derive(Debug, Clone)]
pub enum DocumentProjection {
    /// Select or rename a field: `doc.{ name }`, `doc.{ name: expr }`.
    Field { name: Ident, value: Option<ExprId> },
    /// Spread another document/struct: `doc.{ ...expr }`.
    Spread(ExprId),
}

/// Kind of comprehension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComprehensionKind {
    List,
    Set,
    Dict,
}
