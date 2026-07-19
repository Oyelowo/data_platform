//! Runtime values and columnar batches for query execution.
//!
//! This module defines the executor's internal value representation. It is
//! intentionally simple for the skeleton; Arrow compatibility is planned as an
//! adapter layer on top.

use std::sync::Arc;

use yelang_interner::Symbol;

/// A runtime value produced by query execution.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i128),
    Float(f64),
    Str(Symbol),
    Array(Vec<Value>),
    Record(Vec<(Symbol, Value)>),
    Error(String),
}

/// A column of values.
pub type Column = Vec<Value>;

/// A record batch: a set of named columns with a row count.
#[derive(Clone, Debug, Default)]
pub struct RecordBatch {
    pub columns: Vec<(Symbol, Column)>,
    pub row_count: usize,
}

impl RecordBatch {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn single_column(name: Symbol, values: Vec<Value>) -> Self {
        let row_count = values.len();
        Self {
            columns: vec![(name, values)],
            row_count,
        }
    }
}

impl Value {
    pub fn to_bool(&self) -> bool {
        matches!(self, Value::Bool(true))
    }

    pub fn field(&self, name: Symbol) -> Option<&Value> {
        match self {
            Value::Record(fields) => fields.iter().find(|(n, _)| *n == name).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn try_into_array(self) -> Result<Vec<Value>, String> {
        match self {
            Value::Array(a) => Ok(a),
            _ => Err("expected array".to_string()),
        }
    }

    pub fn try_into_record(self) -> Result<Vec<(Symbol, Value)>, String> {
        match self {
            Value::Record(r) => Ok(r),
            _ => Err("expected record".to_string()),
        }
    }
}

pub fn value_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Float(x), Value::Float(y)) => x.to_bits() == y.to_bits(),
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Array(xs), Value::Array(ys)) => {
            xs.len() == ys.len() && xs.iter().zip(ys).all(|(a, b)| value_eq(a, b))
        }
        (Value::Record(xs), Value::Record(ys)) => {
            xs.len() == ys.len()
                && xs.iter().all(|(n, v)| {
                    ys.iter()
                        .find(|(m, _)| m == n)
                        .map(|(_, w)| value_eq(v, w))
                        .unwrap_or(false)
                })
        }
        _ => false,
    }
}

/// Schema placeholder for Arrow integration.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ArrowSchema {
    pub fields: Vec<ArrowField>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArrowField {
    pub name: Symbol,
    pub nullable: bool,
    pub ty: ArrowType,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArrowType {
    Bool,
    I8,
    I16,
    I32,
    I64,
    I128,
    F32,
    F64,
    Utf8,
    Binary,
    List(Arc<ArrowType>),
    Struct(Vec<ArrowField>),
}
