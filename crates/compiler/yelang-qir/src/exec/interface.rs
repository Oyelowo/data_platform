//! Execution interface for physical QIR plans.

use crate::errors::PlanError;
use crate::physical::PhysicalPlan;

/// A runtime value produced by query execution.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i128),
    Float(f64),
    Str(String),
    Array(Vec<Value>),
    Record(Vec<(String, Value)>),
    Error(String),
}

/// Trait implemented by query executors.
pub trait QueryExecutor {
    type Error;

    /// Execute a physical plan and return its result value.
    fn execute(&self, plan: &PhysicalPlan) -> Result<Value, Self::Error>;
}

impl From<PlanError> for Value {
    fn from(err: PlanError) -> Self {
        Value::Error(err.to_string())
    }
}
