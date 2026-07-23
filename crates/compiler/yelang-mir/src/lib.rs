//! Mid-level Intermediate Representation (MIR) for Yelang.
//!
//! MIR is a low-level, control-flow-graph-based IR used for:
//! - Non-query code: functions, loops, closures, operators, assignments
//! - Query bridge: QIR results become MIR locals
//! - Optimization: dead code elimination, inlining, simplification
//! - Code generation: MIR → LLVM IR / Cranelift / custom backend
//!
//! MIR is NOT used for query expressions — those go through QIR.
//! The QIR↔MIR bridge converts query results into MIR values.
//!
//! Design follows rustc's MIR, simplified for Yelang:
//! - No lifetimes, no borrow checker
//! - No `Place` projections for borrows (Yelang has no `&mut`)
//! - Query expressions handled by QIR, not MIR

pub mod body;
pub mod build;
pub mod ops;
pub mod place;
pub mod terminator;

pub use body::{
    AggregateKind, BasicBlock, BasicBlockData, BinOp, Body, Local, LocalDecl, LocalKind, Rvalue,
    Statement, UnOp,
};
pub use ops::{ConstValue, Constant, Operand};
pub use place::{Place, Projection};
pub use terminator::{AssertKind, SwitchTargets, Terminator, TerminatorKind};
