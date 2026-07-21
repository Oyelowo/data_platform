//! Typed High-level Intermediate Representation (THIR) for Yelang.
//!
//! THIR sits between HIR and QIR. It is fully typed and desugared:
//! method calls are plain function calls with an explicit `self` argument,
//! query syntax is represented by `ThirExpr::Query(QueryId)`, and surface
//! sugar such as `for` loops is already lowered (HIR lowers `for` to
//! `Loop` + `Match`).

pub mod body;
pub mod context;
pub mod errors;
pub mod expr;
pub mod ids;
pub mod lower;
pub mod lower_expr;
pub mod lower_pat;
pub mod lower_stmt;
pub mod pat;
pub mod stmt;
pub mod ty;

pub use body::{ThirBodies, ThirBody};
pub use context::LoweringContext;
pub use errors::LoweringError;
pub use expr::{ThirArm, ThirExpr};
pub use ids::{ThirBodyId, ThirExprId, ThirPatId, ThirStmtId};
pub use lower::lower_body;
pub use pat::ThirPat;
pub use stmt::ThirStmt;
pub use ty::ThirTyId;
