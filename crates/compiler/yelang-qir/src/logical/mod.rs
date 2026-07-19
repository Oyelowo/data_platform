//! Logical QIR: backend-agnostic operators and lowering from typed HIR.

pub mod aggregate;
pub mod iterator;
pub mod links;
pub mod lower;
pub mod lower_expr;
pub mod lower_method;
pub mod lower_query;
pub mod lower_selector;
pub mod operator;
pub mod plan;
pub mod props;
pub mod queryable;
pub mod shape;
pub mod stdlib;

pub use operator::{
    AggregateOp, ConstructKind, EdgeDirection, JoinKind, LirOp, ScanSource, SetOpKind,
};
pub use plan::LogicalPlan;
pub use props::{Boundedness, CardinalityClass, LogicalProps};
pub use shape::{CorrelationMode, NestedShape};
