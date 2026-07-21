//! Physical QIR: operators, properties, planning, and exchanges.

pub mod capability;
pub mod cost;
pub mod enforcers;
pub mod memo;
pub mod operator;
pub mod plan;
pub mod planner;
pub mod props;
pub mod registry;
pub mod rules;
pub mod stats;
pub mod tasks;

pub use operator::{AggMode, ExchangeKind, PhysicalAggregateOp, PirOp, RepartitionKind};
pub use plan::PhysicalPlan;
pub use planner::plan_logical;
pub use cost::Cardinality;
pub use props::{Boundedness, Cost, Location, Partitioning, PhysicalOrdering, PhysicalProps};
