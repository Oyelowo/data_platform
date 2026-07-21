//! Scalar kernel registry for the in-memory executor.

pub mod arithmetic;
pub mod boolean;
pub mod cast;
pub mod compare;
pub mod strings;

use crate::expr::{AggregateClass, QBinaryOp, QUnaryOp};
use crate::exec::interface::Value;
use crate::exec::value::value_eq;

/// Registry of pure scalar operations available to the executor.
#[derive(Debug, Default)]
pub struct KernelRegistry;

impl KernelRegistry {
    /// Create an empty kernel registry.
    pub fn new() -> Self {
        Self
    }

    /// Evaluate a binary operation.
    pub fn eval_binary(&self, op: QBinaryOp, left: Value, right: Value) -> Value {
        match op {
            QBinaryOp::Add => match (left, right) {
                (Value::Int(l), Value::Int(r)) => Value::Int(l + r),
                (Value::Float(l), Value::Float(r)) => Value::Float(l + r),
                _ => Value::Null,
            },
            QBinaryOp::Sub => match (left, right) {
                (Value::Int(l), Value::Int(r)) => Value::Int(l - r),
                (Value::Float(l), Value::Float(r)) => Value::Float(l - r),
                _ => Value::Null,
            },
            QBinaryOp::Mul => match (left, right) {
                (Value::Int(l), Value::Int(r)) => Value::Int(l * r),
                (Value::Float(l), Value::Float(r)) => Value::Float(l * r),
                _ => Value::Null,
            },
            QBinaryOp::Div => match (left, right) {
                (Value::Int(l), Value::Int(r)) if r != 0 => Value::Int(l / r),
                (Value::Float(l), Value::Float(r)) if r != 0.0 => Value::Float(l / r),
                _ => Value::Null,
            },
            QBinaryOp::Gt => match (left, right) {
                (Value::Int(l), Value::Int(r)) => Value::Bool(l > r),
                (Value::Float(l), Value::Float(r)) => Value::Bool(l > r),
                _ => Value::Bool(false),
            },
            QBinaryOp::Lt => match (left, right) {
                (Value::Int(l), Value::Int(r)) => Value::Bool(l < r),
                (Value::Float(l), Value::Float(r)) => Value::Bool(l < r),
                _ => Value::Bool(false),
            },
            QBinaryOp::Gte => match (left, right) {
                (Value::Int(l), Value::Int(r)) => Value::Bool(l >= r),
                (Value::Float(l), Value::Float(r)) => Value::Bool(l >= r),
                _ => Value::Bool(false),
            },
            QBinaryOp::Lte => match (left, right) {
                (Value::Int(l), Value::Int(r)) => Value::Bool(l <= r),
                (Value::Float(l), Value::Float(r)) => Value::Bool(l <= r),
                _ => Value::Bool(false),
            },
            QBinaryOp::Eq => Value::Bool(value_eq(&left, &right)),
            QBinaryOp::Ne => Value::Bool(!value_eq(&left, &right)),
            QBinaryOp::And => Value::Bool(left.to_bool() && right.to_bool()),
            QBinaryOp::Or => Value::Bool(left.to_bool() || right.to_bool()),
            _ => Value::Null,
        }
    }

    /// Evaluate a unary operation.
    pub fn eval_unary(&self, op: QUnaryOp, value: Value) -> Value {
        match op {
            QUnaryOp::Not => Value::Bool(!value.to_bool()),
            QUnaryOp::Neg => match value {
                Value::Int(n) => Value::Int(-n),
                Value::Float(n) => Value::Float(-n),
                _ => Value::Null,
            },
            QUnaryOp::BitNot => Value::Null,
        }
    }

    /// Evaluate an aggregate over a collected set of values.
    pub fn eval_aggregate(&self, class: AggregateClass, values: Vec<Value>) -> Value {
        match class {
            AggregateClass::Distributive | AggregateClass::Algebraic => {
                // Default distributive/algebraic behavior: sum integers, count otherwise.
                let mut sum = 0i128;
                let mut count = 0usize;
                for v in values {
                    if let Value::Int(n) = v {
                        sum += n;
                    }
                    count += 1;
                }
                if count == 0 {
                    Value::Int(0)
                } else {
                    Value::Int(sum)
                }
            }
            AggregateClass::Holistic => Value::Int(values.len() as i128),
        }
    }
}
