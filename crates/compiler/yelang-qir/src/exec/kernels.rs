//! Scalar kernel registry for the in-memory executor.

use crate::expr::{QBinaryOp, QUnaryOp};
use crate::exec::interface::Value;

/// Registry of pure scalar operations available to the executor.
#[derive(Debug, Default)]
pub struct KernelRegistry;

impl KernelRegistry {
    /// Create an empty kernel registry.
    pub fn new() -> Self {
        Self
    }

    /// Evaluate a binary operation.
    pub fn eval_binary(&self, _op: QBinaryOp, left: Value, right: Value) -> Value {
        // Skeleton: return the left operand unchanged.
        let _ = right;
        left
    }

    /// Evaluate a unary operation.
    pub fn eval_unary(&self, _op: QUnaryOp, value: Value) -> Value {
        value
    }
}
