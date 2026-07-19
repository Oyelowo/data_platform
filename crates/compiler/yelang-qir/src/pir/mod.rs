//! Physical QIR: operators, properties, planning, and exchanges.

pub mod aggregations;
pub mod cost;
pub mod enforcers;
pub mod exchanges;
pub mod joins;
pub mod operator;
pub mod plan;
pub mod planner;
pub mod props;

pub use operator::{AggMode, ExchangeKind, PhysicalAggregateOp, PirOp, RepartitionKind};
pub use plan::PhysicalPlan;
pub use planner::plan_logical;
pub use cost::Cardinality;
pub use props::{Boundedness, Cost, Location, Partitioning, PhysicalOrdering, PhysicalProps};
