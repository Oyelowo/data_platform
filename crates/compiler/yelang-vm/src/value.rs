//! Runtime values for the Yelang VM.
//!
//! Values represent all Yelang types at runtime. The VM operates on
//! these values via the bytecode instruction set.

use yelang_interner::Symbol;

/// A runtime value in the VM.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// Unit value `()`.
    Unit,
    /// Boolean.
    Bool(bool),
    /// Signed integer (i8–i128, stored as i128).
    Int(i128),
    /// Unsigned integer (u8–u128, stored as u128).
    Uint(u128),
    /// Float (f32/f64, stored as f64).
    Float(f64),
    /// Character.
    Char(char),
    /// String (interned).
    Str(Symbol),
    /// Array of values.
    Array(Vec<Value>),
    /// Tuple of values.
    Tuple(Vec<Value>),
    /// Struct: (type_def_id, field values).
    Struct(u64, Vec<(Symbol, Value)>),
    /// Enum variant: (type_def_id, variant_index, field values).
    EnumVariant(u64, usize, Vec<Value>),
    /// Option: None or Some(value).
    Option(Option<Box<Value>>),
    /// Result: Ok(value) or Err(value).
    Result(Result<Box<Value>, Box<Value>>),
    /// Closure: (function_id, captured values).
    Closure(u64, Vec<Value>),
    /// Function pointer.
    FnPtr(u64),
    /// Query result: an array of rows (each row is a Struct).
    QueryResult(Vec<Value>),
    /// Iterator: (underlying array, current index).
    Iterator(Vec<Value>, usize),
    /// Null / uninitialized.
    Null,
}

impl Value {
    /// Whether this value is truthy (for conditionals).
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Int(i) => *i != 0,
            Value::Uint(u) => *u != 0,
            Value::Null => false,
            Value::Option(opt) => opt.is_some(),
            _ => true,
        }
    }

    /// Whether this value is null/uninitialized.
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// Get as i128 (for arithmetic).
    pub fn as_int(&self) -> Option<i128> {
        match self {
            Value::Int(i) => Some(*i),
            Value::Uint(u) => Some(*u as i128),
            Value::Bool(b) => Some(if *b { 1 } else { 0 }),
            _ => None,
        }
    }

    /// Get as f64 (for float arithmetic).
    pub fn as_float(&self) -> Option<f64> {
        match self {
            Value::Float(f) => Some(*f),
            Value::Int(i) => Some(*i as f64),
            Value::Uint(u) => Some(*u as f64),
            _ => None,
        }
    }

    /// Get as bool.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Get as usize (for indexing).
    pub fn as_usize(&self) -> Option<usize> {
        match self {
            Value::Int(i) if *i >= 0 => Some(*i as usize),
            Value::Uint(u) => Some(*u as usize),
            _ => None,
        }
    }

    /// Get a struct field by name.
    pub fn get_field(&self, name: Symbol) -> Option<&Value> {
        match self {
            Value::Struct(_, fields) => {
                fields.iter().find(|(n, _)| *n == name).map(|(_, v)| v)
            }
            _ => None,
        }
    }

    /// Set a struct field by name.
    pub fn set_field(&mut self, name: Symbol, value: Value) {
        if let Value::Struct(_, fields) = self {
            if let Some((_, v)) = fields.iter_mut().find(|(n, _)| *n == name) {
                *v = value;
            }
        }
    }

    /// Get an array element by index.
    pub fn index(&self, idx: usize) -> Option<&Value> {
        match self {
            Value::Array(elems) => elems.get(idx),
            Value::QueryResult(rows) => rows.get(idx),
            _ => None,
        }
    }

    /// Get the length of an array/string.
    pub fn len(&self) -> Option<usize> {
        match self {
            Value::Array(elems) => Some(elems.len()),
            Value::QueryResult(rows) => Some(rows.len()),
            Value::Tuple(elems) => Some(elems.len()),
            _ => None,
        }
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Unit => write!(f, "()"),
            Value::Bool(b) => write!(f, "{}", b),
            Value::Int(i) => write!(f, "{}", i),
            Value::Uint(u) => write!(f, "{}", u),
            Value::Float(fl) => write!(f, "{}", fl),
            Value::Char(c) => write!(f, "'{}'", c),
            Value::Str(s) => write!(f, "\"{:?}\"", s),
            Value::Array(elems) => {
                write!(f, "[")?;
                for (i, elem) in elems.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", elem)?;
                }
                write!(f, "]")
            }
            Value::Tuple(elems) => {
                write!(f, "(")?;
                for (i, elem) in elems.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", elem)?;
                }
                write!(f, ")")
            }
            Value::Struct(_, fields) => {
                write!(f, "{{ ")?;
                for (i, (name, val)) in fields.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{:?}: {}", name, val)?;
                }
                write!(f, " }}")
            }
            Value::EnumVariant(_, idx, vals) => {
                write!(f, "Variant{}(", idx)?;
                for (i, val) in vals.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", val)?;
                }
                write!(f, ")")
            }
            Value::Option(opt) => match opt {
                Some(val) => write!(f, "Some({})", val),
                None => write!(f, "None"),
            },
            Value::Result(res) => match res {
                Ok(val) => write!(f, "Ok({})", val),
                Err(val) => write!(f, "Err({})", val),
            },
            Value::Closure(id, _) => write!(f, "<closure:{}>", id),
            Value::FnPtr(id) => write!(f, "<fn:{}>", id),
            Value::QueryResult(rows) => {
                write!(f, "QueryResult[{} rows]", rows.len())
            }
            Value::Iterator(_, idx) => write!(f, "<iterator at {}>", idx),
            Value::Null => write!(f, "null"),
        }
    }
}
