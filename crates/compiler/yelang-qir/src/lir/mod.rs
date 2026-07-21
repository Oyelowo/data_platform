//! Logical QIR: backend-agnostic operators and lowering from typed HIR.

pub mod aggregate;
pub mod aggregate_impl;
pub mod analyze;
pub mod iterator;
pub mod lower;
pub mod operator;
pub mod plan;
pub mod props;
pub mod shape;
pub mod stdlib;
pub mod window;

pub use operator::{
    AggregateOp, ConstructKind, EdgeDirection, JoinKind, LirOp, ScanSource, SetOpKind,
};
pub use plan::LogicalPlan;
pub use props::{Boundedness, CardinalityClass, LogicalProps};
pub use shape::{CorrelationMode, NestedShape};
